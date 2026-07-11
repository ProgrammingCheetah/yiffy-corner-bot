//! Boot configuration (environment) and the shared application state that
//! dptree injects into every handler.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use domain::elements::user::TelegramId;
use infra_e621::RateLimitedE621Client;
use persistence::sqlite::{
    announcement::SqliteAnnouncementRepository,
    post::SqlitePostRepository,
    poster::SqlitePosterRepository,
    publication::SqlitePublicationRepository,
    publisher_config::SqlitePublisherConfigRepository,
    report::SqliteReportRepository,
    scoreboard::SqliteScoreboardRepository,
    tag_policy::{
        SqliteForbiddenTagRepository, SqliteRequiredTagRepository, SqliteSpoilerTagRepository,
    },
    skiplist::SqliteSkipListRepository,
    telegram_copy::SqliteTelegramCopyRepository,
    user::SqliteUserRepository,
};

/// App version — keep in step with `webapp/src/lib/changelog.js` (the
/// changelog is the user-facing face of the same number).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const USER_AGENT: &str = concat!(
    "yiffy-corner-bot/",
    env!("CARGO_PKG_VERSION"),
    " (by ZieloAnima on e621)"
);

/// Environment-derived configuration.
///
/// - `YCB_ENV`: vault environment folder (`development` default).
/// - `YCB_VAULT_DIR`: vault root (default `config/vault`, the legacy layout).
/// - `YCB_DATABASE_URL`: sqlite URL (default `<vault>/storage/rust-bot.sqlite`).
/// - `YCB_OWNER_ID`: seed Owner Telegram ID (default Zuri).
/// - `YCB_HEALTH_ADDR`: health endpoint bind (default `0.0.0.0:3000`).
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub vault_env_dir: PathBuf,
    pub database_url: String,
    pub owner_id: TelegramId,
    pub health_addr: String,
    /// Public HTTPS URL of the Mini App (sets the bot's menu button).
    pub webapp_url: Option<String>,
    /// Directory with the built SvelteKit bundle to serve.
    pub webapp_dir: Option<PathBuf>,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let env = std::env::var("YCB_ENV").unwrap_or_else(|_| "development".to_string());
        let vault_root =
            PathBuf::from(std::env::var("YCB_VAULT_DIR").unwrap_or_else(|_| "config/vault".into()));
        let database_url = std::env::var("YCB_DATABASE_URL").unwrap_or_else(|_| {
            format!(
                "sqlite:{}",
                vault_root.join("storage/rust-bot.sqlite").display()
            )
        });
        let owner_id = std::env::var("YCB_OWNER_ID")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(1402476143);
        let health_addr =
            std::env::var("YCB_HEALTH_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
        let webapp_url = std::env::var("YCB_WEBAPP_URL")
            .ok()
            .filter(|v| !v.is_empty());
        let webapp_dir = std::env::var("YCB_WEBAPP_DIR")
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from("webapp/build")));
        Self {
            vault_env_dir: vault_root.join(env),
            database_url,
            owner_id: TelegramId::from(owner_id),
            health_addr,
            webapp_url,
            webapp_dir,
        }
    }

    pub fn token_path(&self) -> PathBuf {
        self.vault_env_dir.join("token.txt")
    }
}

/// Read a one-line secret file (trailing whitespace stripped).
pub fn read_secret(path: &Path) -> std::io::Result<String> {
    Ok(std::fs::read_to_string(path)?.trim().to_string())
}

/// A submission waiting for its tags (feed model: every entry is tagged).
/// Keyed by the submitter's Telegram id; their next plain-text message
/// completes it. In-memory: a restart just means re-submitting.
#[derive(Debug, Clone)]
pub struct PendingSubmission {
    pub url: url::Url,
    /// Present when the submission arrived as a channel forward — carries
    /// what the copy-ref store needs once tags arrive.
    pub forward: Option<PendingForward>,
    /// Moderator /save: tags complete a DIRECT feed add, not a suggestion.
    pub direct_add: bool,
}

#[derive(Debug, Clone)]
pub struct PendingForward {
    pub origin_chat_id: i64,
    pub origin_message_id: i32,
    pub channel_username: String,
}

/// The concrete dependency bundle the scheduler AND the out-of-band pool
/// batch publish through — one instance, shared, so both paths use the same
/// publisher cache and repositories.
pub type PublishDeps = application::actors::scheduler::SchedulerDeps<
    SqlitePostRepository,
    SqliteUserRepository,
    SqlitePosterRepository,
    SqlitePublicationRepository,
    SqliteSpoilerTagRepository,
>;

/// Everything the command handlers need. One `Arc` in dptree deps.
pub struct AppState {
    pub config: AppConfig,
    pub users: SqliteUserRepository,
    pub posts: SqlitePostRepository,
    pub posters: SqlitePosterRepository,
    pub publisher_configs: SqlitePublisherConfigRepository,
    pub forbidden: SqliteForbiddenTagRepository,
    pub required: SqliteRequiredTagRepository,
    pub spoilers: SqliteSpoilerTagRepository,
    pub telegram_copies: SqliteTelegramCopyRepository,
    pub reports: SqliteReportRepository,
    /// Browse skiplist: sources moderators waved off for good.
    pub skips: SqliteSkipListRepository,
    pub publications: SqlitePublicationRepository,
    pub announcements: SqliteAnnouncementRepository,
    pub scoreboards: SqliteScoreboardRepository,
    pub e621: Arc<RateLimitedE621Client>,
    /// The same composite media resolver the scheduler publishes with —
    /// review DMs resolve real media through it too.
    pub resolver: Arc<crate::resolvers::CompositeResolver>,
    /// Perceptual hasher for the duplicate check on submissions.
    pub hasher: Arc<dyn domain::elements::phash::PerceptualHasher>,
    /// The publish pipeline (same instance the scheduler runs on), for
    /// out-of-band publishes like whole-pool batches.
    pub publish_deps: PublishDeps,
    /// Submissions awaiting tags, keyed by submitter Telegram id.
    pub pending: tokio::sync::Mutex<std::collections::HashMap<i64, PendingSubmission>>,
    /// In-flight moderation dialogues, keyed by moderator Telegram id:
    /// their next message completes the action.
    pub pending_moderation: tokio::sync::Mutex<std::collections::HashMap<i64, ModerationDialogue>>,
    /// Viewer reports awaiting their reason, keyed by reporter Telegram id:
    /// their next message files the report. In-memory like `pending` — a
    /// restart just means pressing Report again.
    pub pending_reports: tokio::sync::Mutex<std::collections::HashMap<i64, PendingReport>>,
    /// "More like this" wishes awaiting their text, keyed by requester
    /// Telegram id — same dialogue shape as `pending_reports`, but expiring
    /// silently (an unanswered wish just evaporates; nothing to file).
    pub pending_more: tokio::sync::Mutex<
        std::collections::HashMap<i64, (domain::elements::post::PostId, std::time::Instant)>,
    >,
    /// Last /browse query per moderator, for the "More ➡" paging button.
    pub browse_sessions: tokio::sync::Mutex<std::collections::HashMap<i64, BrowseSession>>,
}

/// A moderator's paging position within their last /browse query.
#[derive(Debug, Clone)]
pub struct BrowseSession {
    pub tags: Vec<domain::elements::tag::Tag>,
    /// The NEXT e621 page to fetch.
    pub next_page: u32,
    pub count: usize,
}

/// A viewer report awaiting its reason.
#[derive(Debug, Clone)]
pub struct PendingReport {
    pub post_id: domain::elements::post::PostId,
    /// When the dialogue was armed — after `REPORT_REASON_TIMEOUT` the
    /// report files reasonless, and this Instant tells the timeout task
    /// whether the entry is still *its* dialogue.
    pub armed_at: std::time::Instant,
    /// The reporter's live @username, captured at arming so the filed
    /// report always carries a contact.
    pub username: Option<String>,
}

/// What a moderator's next message means.
#[derive(Debug, Clone, Copy)]
pub enum ModerationDialogue {
    /// Next message = the rejection reason (relayed to the submitter).
    RejectReason(domain::elements::post::PostId),
    /// Next message = extra tags to merge before accepting into the feed.
    ExtraTags(domain::elements::post::PostId),
    /// Next message = the changes the submitter should make (relayed to
    /// them; they can then re-submit the same source).
    RequestChanges(domain::elements::post::PostId),
}

pub type SharedState = Arc<AppState>;
