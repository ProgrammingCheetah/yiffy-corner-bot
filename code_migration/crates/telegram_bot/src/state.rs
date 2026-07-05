//! Boot configuration (environment) and the shared application state that
//! dptree injects into every handler.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use domain::elements::user::TelegramId;
use infra_e621::RateLimitedE621Client;
use persistence::sqlite::{
    post::SqlitePostRepository,
    poster::SqlitePosterRepository,
    publisher_config::SqlitePublisherConfigRepository,
    tag_policy::{SqliteForbiddenTagRepository, SqliteRequiredTagRepository},
    user::SqliteUserRepository,
};

pub const USER_AGENT: &str = "yiffy-corner-bot/0.2 (by ZielAnima)";

/// Environment-derived configuration.
///
/// - `YCB_ENV`: vault environment folder (`development` default).
/// - `YCB_VAULT_DIR`: vault root (default `config/vault`, the legacy layout).
/// - `YCB_DATABASE_URL`: sqlite URL (default `<vault>/storage/rust-bot.sqlite`).
/// - `YCB_OWNER_ID`: seed Owner Telegram ID (default Zuri).
/// - `YCB_HEALTH_ADDR`: health endpoint bind (default `0.0.0.0:3000`).
/// - `YCB_REPOST_COOLDOWN_DAYS`: selector repost cooldown (default 7).
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub vault_env_dir: PathBuf,
    pub database_url: String,
    pub owner_id: TelegramId,
    pub health_addr: String,
    pub repost_cooldown: chrono::Duration,
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
        let cooldown_days = std::env::var("YCB_REPOST_COOLDOWN_DAYS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(7);
        Self {
            vault_env_dir: vault_root.join(env),
            database_url,
            owner_id: TelegramId::from(owner_id),
            health_addr,
            repost_cooldown: chrono::Duration::days(cooldown_days),
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

/// Everything the command handlers need. One `Arc` in dptree deps.
pub struct AppState {
    pub config: AppConfig,
    pub users: SqliteUserRepository,
    pub posts: SqlitePostRepository,
    pub posters: SqlitePosterRepository,
    pub publisher_configs: SqlitePublisherConfigRepository,
    pub forbidden: SqliteForbiddenTagRepository,
    pub required: SqliteRequiredTagRepository,
    pub e621: Arc<RateLimitedE621Client>,
}

pub type SharedState = Arc<AppState>;
