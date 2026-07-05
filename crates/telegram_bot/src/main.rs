mod announcer;
mod commands;
mod publishers;
mod resolvers;
mod state;

use std::sync::Arc;

use application::actors::scheduler::{SchedulerDeps, start_scheduler};
use application::selectors::feed::FeedSelectorFactory;
use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use domain::elements::user::{Role, UserRepository as _};
use infra_e621::RateLimitedE621Client;
use infra_fixup::FixupResolver;
use infra_furaffinity::{FaCookies, FuraffinityResolver};
use persistence::sqlite::{
    self,
    post::SqlitePostRepository,
    poster::SqlitePosterRepository,
    publication::SqlitePublicationRepository,
    publisher_config::SqlitePublisherConfigRepository,
    report::SqliteReportRepository,
    tag_policy::{
        SqliteForbiddenTagRepository, SqliteRequiredTagRepository, SqliteSpoilerTagRepository,
    },
    user::SqliteUserRepository,
};
use teloxide::{Bot, prelude::*, utils::command::BotCommands as _};

use telemetry::Event;

use crate::commands::{Command, handle_callback, handle_channel_forward, handle_command};
use crate::publishers::DbPublisherFactory;
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
    let mut e621_client =
        RateLimitedE621Client::new(USER_AGENT).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    // Optional e621 API credentials (vault: e621_login.txt + e621_key.txt).
    if let (Ok(login), Ok(api_key)) = (
        read_secret(&config.vault_env_dir.join("e621_login.txt")),
        read_secret(&config.vault_env_dir.join("e621_key.txt")),
    ) {
        e621_client = e621_client.with_credentials(infra_e621::E621Credentials { login, api_key });
    }
    let e621 = Arc::new(e621_client);
    let telegram_copies =
        persistence::sqlite::telegram_copy::SqliteTelegramCopyRepository::new(pool.clone());
    let resolver = build_resolver(&config, e621.clone(), telegram_copies.clone())?;
    let state = Arc::new(AppState {
        users: SqliteUserRepository::new(pool.clone()),
        posts: SqlitePostRepository::new(pool.clone()),
        posters: SqlitePosterRepository::new(pool.clone()),
        publisher_configs: SqlitePublisherConfigRepository::new(pool.clone()),
        forbidden: SqliteForbiddenTagRepository::new(pool.clone()),
        required: SqliteRequiredTagRepository::new(pool.clone()),
        spoilers: SqliteSpoilerTagRepository::new(pool.clone()),
        telegram_copies: telegram_copies.clone(),
        reports: SqliteReportRepository::new(pool.clone()),
        publications: SqlitePublicationRepository::new(pool.clone()),
        announcements: persistence::sqlite::announcement::SqliteAnnouncementRepository::new(
            pool.clone(),
        ),
        e621: e621.clone(),
        resolver: resolver.clone(),
        config: config.clone(),
        pending: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        pending_moderation: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        browse_sessions: tokio::sync::Mutex::new(std::collections::HashMap::new()),
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
    // The bot's @username feeds the Report deep links in captions.
    let bot_username = bot
        .get_me()
        .await
        .ok()
        .and_then(|me| me.username.clone())
        .unwrap_or_default();

    // Scheduler, database-first: posters, tags, cursors and channel bindings
    // are read fresh every tick — /newposter, /settags and /setchannel are
    // live within a minute, no restarts.
    tracing::info!(event = %Event::RuntimesLoaded, "scheduler running database-first");
    tokio::spawn(start_scheduler(SchedulerDeps {
        posts: Arc::new(state.posts.clone()),
        users: Arc::new(state.users.clone()),
        posters: Arc::new(state.posters.clone()),
        publications: Arc::new(state.publications.clone()),
        spoilers: Arc::new(state.spoilers.clone()),
        selectors: Arc::new(FeedSelectorFactory {
            posts: Arc::new(state.posts.clone()),
            e621: e621.clone(),
            forbidden: Arc::new(state.forbidden.clone()),
        }),
        publishers: Arc::new(DbPublisherFactory::new(
            state.publisher_configs.clone(),
            bot.clone(),
            main_token.clone(),
        )),
        resolver,
        bot_username,
    }));

    // Announcement cycle (channel directory broadcasts).
    tokio::spawn(announcer::run(state.clone(), bot.clone()));

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
        .branch(Update::filter_message().endpoint(commands::handle_pending_tags))
        .branch(Update::filter_callback_query().endpoint(handle_callback));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
    Ok(())
}
