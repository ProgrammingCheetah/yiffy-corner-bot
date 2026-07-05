//! Composition-root adapters: the per-source media dispatch and the
//! Telegram username resolver.

use async_trait::async_trait;
use domain::elements::{
    media::{MediaResolveError, MediaResolver, ResolvedMedia},
    post::Source,
    telegram::{ResolveError, TelegramUserResolver},
    user::TelegramId,
};
use infra_e621::RateLimitedE621Client;
use infra_fixup::FixupResolver;
use infra_furaffinity::FuraffinityResolver;
use std::sync::Arc;
use teloxide::{Bot, prelude::Requester, types::Recipient};

/// Dispatches a [`Source`] to the platform resolver that owns it, so the rest
/// of the system holds exactly one `dyn MediaResolver`.
pub struct CompositeResolver {
    pub e621: Arc<RateLimitedE621Client>,
    pub fixup: FixupResolver,
    pub furaffinity: FuraffinityResolver,
}

#[async_trait]
impl MediaResolver for CompositeResolver {
    async fn resolve(&self, source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
        match source {
            Source::E621(_) => self.e621.resolve(source).await,
            Source::FurAffinity(_) => self.furaffinity.resolve(source).await,
            Source::Twitter(_) | Source::BlueSky(_) | Source::DeviantArt(_)
            | Source::Telegram(_) => self.fixup.resolve(source).await,
        }
    }
}

/// Resolves `@username` → [`TelegramId`] via the Bot API.
///
/// Numeric strings pass straight through (the Bot API cannot look up user
/// `@handle`s, only channels — so privileged commands accept raw IDs too).
pub struct BotUserResolver {
    pub bot: Bot,
}

#[async_trait]
impl TelegramUserResolver for BotUserResolver {
    async fn resolve_username(&self, username: &str) -> Result<Option<TelegramId>, ResolveError> {
        let trimmed = username.trim_start_matches('@');
        if let Ok(id) = trimmed.parse::<i64>() {
            return Ok(Some(TelegramId::from(id)));
        }
        match self
            .bot
            .get_chat(Recipient::ChannelUsername(format!("@{trimmed}")))
            .await
        {
            Ok(chat) => Ok(Some(TelegramId::from(chat.id.0))),
            // The Bot API answers "chat not found" for user handles it cannot
            // see; that is "unknown", not a transport failure.
            Err(teloxide::RequestError::Api(_)) => Ok(None),
            Err(e) => Err(ResolveError::Telegram(e.to_string())),
        }
    }
}
