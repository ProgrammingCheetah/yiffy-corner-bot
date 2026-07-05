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
    poster::PosterRepository as _, publisher_config::PublisherConfigRepository as _,
    user::{Role, UserRepository as _},
};
use infra_e621::RateLimitedE621Client;
use infra_fixup::FixupResolver;
use infra_furaffinity::{FaCookies, FuraffinityResolver};
use persistence::sqlite::{
    self, post::SqlitePostRepository, poster::SqlitePosterRepository,
    publisher_config::SqlitePublisherConfigRepository,
    tag_policy::{SqliteForbiddenTagRepository, SqliteRequiredTagRepository},
    user::SqliteUserRepository,
};
use teloxide::{Bot, prelude::*, types::ChatId};

use crate::commands::{Command, handle_callback, handle_command};
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
) -> anyhow::Result<Arc<CompositeResolver>> {
    let cookies = match (
        read_secret(&config.vault_env_dir.join("cookie_a.txt")),
        read_secret(&config.vault_env_dir.join("cookie_b.txt")),
    ) {
        (Ok(a), Ok(b)) => Some(FaCookies { a, b }),
        _ => {
            tracing::warn!(
                "no FA cookies in {} — FurAffinity limited to General-rated content",
                config.vault_env_dir.display()
            );
            None
        }
    };
    Ok(Arc::new(CompositeResolver {
        e621,
        fixup: FixupResolver::new(USER_AGENT).map_err(|e| anyhow::anyhow!(e.to_string()))?,
        furaffinity: FuraffinityResolver::new(USER_AGENT, cookies)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
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
            tracing::warn!(poster = %poster.id, "poster has no channel binding; skipping");
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,teloxide=warn".into()),
        )
        .init();

    let config = AppConfig::from_env();
    tracing::info!(?config, "booting");

    let pool = sqlite::connect_and_migrate(&config.database_url).await?;
    let e621 = Arc::new(
        RateLimitedE621Client::new(USER_AGENT).map_err(|e| anyhow::anyhow!(e.to_string()))?,
    );
    let state = Arc::new(AppState {
        users: SqliteUserRepository::new(pool.clone()),
        posts: SqlitePostRepository::new(pool.clone()),
        posters: SqlitePosterRepository::new(pool.clone()),
        publisher_configs: SqlitePublisherConfigRepository::new(pool.clone()),
        forbidden: SqliteForbiddenTagRepository::new(pool.clone()),
        required: SqliteRequiredTagRepository::new(pool),
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
        tracing::info!(owner = ?config.owner_id, "seeded Owner");
    }

    let main_token = read_secret(&config.token_path())?;
    let bot = Bot::new(main_token.clone());

    // Scheduler: one runtime per bound Poster.
    let resolver = build_resolver(&config, e621)?;
    let runtimes = build_runtimes(&state, resolver, &bot, &main_token).await?;
    tracing::info!(count = runtimes.len(), "scheduler runtimes loaded");
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
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!(error = %e, "health server crashed");
                }
            }
            Err(e) => tracing::error!(error = %e, addr = %health_addr, "health bind failed"),
        }
    });

    // The command dispatcher runs the foreground.
    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .filter_command::<Command>()
                .endpoint(handle_command),
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
