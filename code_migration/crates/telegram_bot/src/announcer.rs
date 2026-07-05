//! The announcement cycle: on the configured cadence, publish the channel
//! directory — every consuming channel, by name, hyperlinked, alphabetical,
//! Spotlight on top — to every consuming channel.

use std::collections::BTreeMap;

use chrono::Utc;
use domain::elements::announcement::AnnouncementRepository as _;
use domain::elements::publisher_config::PublisherConfigRepository as _;
use telemetry::Event;
use teloxide::{
    Bot,
    payloads::SendMessageSetters,
    prelude::Requester,
    types::{ChatId, LinkPreviewOptions, ParseMode},
};

use crate::state::SharedState;

fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

struct DirectoryEntry {
    chat_id: i64,
    title: String,
    link: Option<String>,
}

impl DirectoryEntry {
    fn to_html(&self) -> String {
        match &self.link {
            Some(link) => format!("<a href=\"{link}\">{}</a>", escape_html(&self.title)),
            None => format!("<b>{}</b>", escape_html(&self.title)),
        }
    }
}

/// One announcement round: build the directory and send it everywhere.
/// Returns (sent, failed) counts; `Err` only when there is nothing to do.
pub async fn announce_round(state: &SharedState, bot: &Bot) -> Result<(usize, usize), String> {
    // Distinct consuming chats (several posters may share one channel).
    let configs = state
        .publisher_configs
        .list_all()
        .await
        .map_err(|e| e.to_string())?;
    let chat_ids: Vec<i64> = {
        let mut seen = BTreeMap::new();
        for config in &configs {
            seen.insert(config.chat_id, ());
        }
        seen.into_keys().collect()
    };
    if chat_ids.is_empty() {
        return Err("no consuming channels are bound".to_string());
    }

    // Resolve names + links. Channels the bot can't inspect are skipped.
    let mut entries = Vec::new();
    for chat_id in &chat_ids {
        match bot.get_chat(ChatId(*chat_id)).await {
            Ok(chat) => {
                let title = chat
                    .title()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("Channel {chat_id}"));
                let link = chat
                    .username()
                    .map(|handle| format!("https://t.me/{handle}"))
                    .or_else(|| chat.invite_link().map(ToString::to_string));
                entries.push(DirectoryEntry {
                    chat_id: *chat_id,
                    title,
                    link,
                });
            }
            Err(e) => {
                tracing::warn!(
                    event = %Event::AnnouncementFailed, chat_id, error = %e,
                    "could not resolve channel for the directory"
                );
            }
        }
    }
    if entries.is_empty() {
        return Err("no channels could be resolved".to_string());
    }

    // Alphabetical, Spotlight extracted to the top.
    let spotlight_chat = state
        .announcements
        .get()
        .await
        .map_err(|e| e.to_string())?
        .spotlight_chat_id;
    entries.sort_by_key(|entry| entry.title.to_lowercase());
    let (spotlight, rest): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .partition(|entry| Some(entry.chat_id) == spotlight_chat);

    let mut lines = vec!["📣 <b>Channels on this network:</b>".to_string()];
    for entry in &spotlight {
        lines.push(format!("⭐ {}", entry.to_html()));
    }
    for entry in &rest {
        lines.push(entry.to_html());
    }
    let text = lines.join("\n");

    // Deliver to every consuming chat.
    let (mut sent, mut failed) = (0usize, 0usize);
    for chat_id in &chat_ids {
        match bot
            .send_message(ChatId(*chat_id), text.clone())
            .parse_mode(ParseMode::Html)
            .link_preview_options(LinkPreviewOptions {
                is_disabled: true,
                url: None,
                prefer_small_media: false,
                prefer_large_media: false,
                show_above_text: false,
            })
            .await
        {
            Ok(_) => {
                sent += 1;
                tracing::info!(event = %Event::AnnouncementSent, chat_id, "announcement delivered");
            }
            Err(e) => {
                failed += 1;
                tracing::warn!(event = %Event::AnnouncementFailed, chat_id, error = %e, "announcement delivery failed");
            }
        }
    }
    if let Err(e) = state.announcements.mark_announced(Utc::now()).await {
        tracing::error!(event = %Event::AnnouncementFailed, error = %e, "could not record announcement time");
    }
    Ok((sent, failed))
}

/// Minute loop: fire a round whenever the configured recurrence says so.
pub async fn run(state: SharedState, bot: Bot) -> ! {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let due = match state.announcements.get().await {
            Ok(settings) => settings.due(Utc::now()),
            Err(e) => {
                tracing::error!(event = %Event::AnnouncementFailed, error = %e, "settings read failed");
                false
            }
        };
        if due && let Err(reason) = announce_round(&state, &bot).await {
            tracing::debug!(event = %Event::AnnouncementFailed, reason, "round skipped");
        }
    }
}
