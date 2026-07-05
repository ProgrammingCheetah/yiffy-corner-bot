mod commands;
mod publishers;
mod resolvers;
mod state;

use std::sync::Arc;

use application::actors::scheduler::{PosterRuntime, SchedulerDeps, start_scheduler};
use application::selectors::queue_first::QueueFirstSelector;
use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use domain::elements::{
    poster::PosterRepository as _,
    publisher_config::PublisherConfigRepository as _,
    user::{Role, UserRepository as _},
};
use infra_e621::RateLimitedE621Client;
use infra_fixup::FixupResolver;
use infra_furaffinity::{FaCookies, FuraffinityResolver};
use persistence::sqlite::{
    self,
    post::SqlitePostRepository,
    poster::SqlitePosterRepository,
    publisher_config::SqlitePublisherConfigRepository,
    tag_policy::{SqliteForbiddenTagRepository, SqliteRequiredTagRepository},
    user::SqliteUserRepository,
};
use teloxide::{Bot, prelude::*, types::ChatId, utils::command::BotCommands as _};

use telemetry::Event;

use crate::commands::{Command, handle_callback, handle_channel_forward, handle_command};
use crate::publishers::TelegramPublisher;
use crate::resolvers::CompositeResolver;
use crate::state::{AppConfig, AppState, USER_AGENT, read_secret};

async fn health() -> impl IntoResponse {
    StatusCode::OK
}

/// Build one composite media resolver. FA cookies are optional
/// (`cookie_a.txt` / `cookie_b.txt` in the vault env dir — the legacy names).
fn build_resolver(
    config: &AppConfig,
    e621: Arc<RateLimitedE621Client>,
    telegram_copies: persistence::sqlite::telegram_copy::SqliteTelegramCopyRepository,
) -> anyhow::Result<Arc<CompositeResolver>> {
    let cookies = match (
        read_secret(&config.vault_env_dir.join("cookie_a.txt")),
        read_secret(&config.vault_env_dir.join("cookie_b.txt")),
    ) {
        (Ok(a), Ok(b)) => Some(FaCookies { a, b }),
        _ => {
            tracing::warn!(
                event = %Event::FaCookiesMissing,
                vault_env_dir = %config.vault_env_dir.display(),
                "no FA cookies — FurAffinity limited to General-rated content"
            );
            None
        }
    };
    Ok(Arc::new(CompositeResolver {
        e621,
        fixup: FixupResolver::new(USER_AGENT).map_err(|e| anyhow::anyhow!(e.to_string()))?,
        furaffinity: FuraffinityResolver::new(USER_AGENT, cookies)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        telegram_copies,
    }))
}

/// Load every bound Poster into a scheduler runtime.
async fn build_runtimes(
    state: &AppState,
    resolver: Arc<CompositeResolver>,
    main_bot: &Bot,
    main_token: &str,
) -> anyhow::Result<Vec<PosterRuntime>> {
    let mut runtimes = Vec::new();
    for poster in state
        .posters
        .list_all()
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
    {
        let Some(config) = state
            .publisher_configs
            .find_by_poster(poster.id)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
        else {
            tracing::warn!(event = %Event::PosterUnbound, poster_id = %poster.id, "poster has no channel binding; skipping");
            continue;
        };
        // Posters may publish through their own bot token; the MVP flow binds
        // them all to the main token, so reuse the running Bot in that case.
        let token = read_secret(&config.token_path)?;
        let bot = if token == main_token {
            main_bot.clone()
        } else {
            Bot::new(token)
        };
        let selector = QueueFirstSelector::new(
            poster.clone(),
            Arc::new(state.posts.clone()),
            state.e621.clone(),
            Arc::new(state.forbidden.clone()),
            state.config.repost_cooldown,
        );
        runtimes.push(PosterRuntime {
            poster,
            selector: Box::new(selector),
            resolver: resolver.clone(),
            publisher: Box::new(TelegramPublisher::new(bot, ChatId(config.chat_id))),
        });
    }
    Ok(runtimes)
}

/// JSON lines by default (one object per line, event fields flattened to the
/// top level for `jq`); `YCB_LOG_FORMAT=pretty` switches to the human format
/// for local runs. Level via `RUST_LOG` (default `info,teloxide=warn`).
fn init_logging() {
    let filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,teloxide=warn".into())
    };
    match std::env::var("YCB_LOG_FORMAT").as_deref() {
        Ok("pretty") => tracing_subscriber::fmt().with_env_filter(filter()).init(),
        _ => tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .with_env_filter(filter())
            .init(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();

    let config = AppConfig::from_env();
    tracing::info!(event = %Event::Booting, ?config, "booting");

    let pool = sqlite::connect_and_migrate(&config.database_url).await?;
    let e621 = Arc::new(
        RateLimitedE621Client::new(USER_AGENT).map_err(|e| anyhow::anyhow!(e.to_string()))?,
    );
    let telegram_copies =
        persistence::sqlite::telegram_copy::SqliteTelegramCopyRepository::new(pool.clone());
    let state = Arc::new(AppState {
        users: SqliteUserRepository::new(pool.clone()),
        posts: SqlitePostRepository::new(pool.clone()),
        posters: SqlitePosterRepository::new(pool.clone()),
        publisher_configs: SqlitePublisherConfigRepository::new(pool.clone()),
        forbidden: SqliteForbiddenTagRepository::new(pool.clone()),
        required: SqliteRequiredTagRepository::new(pool),
        telegram_copies: telegram_copies.clone(),
        e621: e621.clone(),
        config: config.clone(),
    });

    // Seed the singleton Owner (Zuri) so privileged commands work from boot.
    if state
        .users
        .find_by_telegram_id(config.owner_id)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .is_none()
    {
        state
            .users
            .create(config.owner_id, Role::Owner, None, None)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        tracing::info!(event = %Event::OwnerSeeded, owner = config.owner_id.as_ref(), "seeded Owner");
    }

    let main_token = read_secret(&config.token_path())?;
    let bot = Bot::new(main_token.clone());

    // Publish the command menu so Telegram clients autocomplete them.
    if let Err(e) = bot.set_my_commands(Command::bot_commands()).await {
        tracing::warn!(error = %e, "set_my_commands failed; menu may be stale");
    }

    // Scheduler: one runtime per bound Poster.
    let resolver = build_resolver(&config, e621, telegram_copies)?;
    let runtimes = build_runtimes(&state, resolver, &bot, &main_token).await?;
    tracing::info!(event = %Event::RuntimesLoaded, count = runtimes.len(), "scheduler runtimes loaded");
    tokio::spawn(start_scheduler(SchedulerDeps {
        runtimes,
        posts: Arc::new(state.posts.clone()),
        users: Arc::new(state.users.clone()),
    }));

    // Health endpoint for container checks.
    let health_addr = config.health_addr.clone();
    tokio::spawn(async move {
        let app = Router::new().route("/health", get(health));
        match tokio::net::TcpListener::bind(&health_addr).await {
            Ok(listener) => {
                tracing::info!(event = %Event::HealthServerUp, addr = %health_addr, "health endpoint listening");
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!(event = %Event::HealthServerFailed, error = %e, "health server crashed");
                }
            }
            Err(e) => {
                tracing::error!(event = %Event::HealthServerFailed, error = %e, addr = %health_addr, "health bind failed")
            }
        }
    });

    // The command dispatcher runs the foreground. Channel forwards are the
    // non-command submission path (checked after commands).
    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .filter_command::<Command>()
                .endpoint(handle_command),
        )
        .branch(
            Update::filter_message()
                .filter(|msg: Message| {
                    matches!(
                        msg.forward_origin(),
                        Some(teloxide::types::MessageOrigin::Channel { .. })
                    )
                })
                .endpoint(handle_channel_forward),
        )
        .branch(Update::filter_callback_query().endpoint(handle_callback));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
    Ok(())
}
