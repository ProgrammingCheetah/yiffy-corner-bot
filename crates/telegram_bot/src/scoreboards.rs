//! The scoreboard cycle: on the configured cadence, every consuming channel
//! receives ITS OWN community leaderboard — who got the most content
//! published there. Because a channel only publishes what matches its tag
//! subscription, each board reflects that channel's taste; staff never rank.

use std::collections::BTreeSet;

use application::commands::scoreboard::channel_board;
use chrono::Utc;
use domain::elements::publisher_config::PublisherConfigRepository as _;
use domain::elements::scoreboard::ScoreboardRepository as _;
use telemetry::Event;
use teloxide::{Bot, prelude::Requester, types::ChatId};

use crate::state::SharedState;

const BOARD_SIZE: usize = 10;

fn board_text(board: &[(domain::elements::user::User, u64)]) -> String {
    let mut lines = vec!["🏆 This channel's top contributors".to_string()];
    for (rank, (user, score)) in board.iter().enumerate() {
        let name = user
            .display_name
            .clone()
            .unwrap_or_else(|| format!("user {}", user.telegram_id.as_ref()));
        let medal = match rank {
            0 => "🥇".to_string(),
            1 => "🥈".to_string(),
            2 => "🥉".to_string(),
            n => format!("{}.", n + 1),
        };
        let noun = if *score == 1 { "post" } else { "posts" };
        lines.push(format!("{medal} {name} — {score} {noun}"));
    }
    lines.join("\n")
}

/// One scoreboard round: each consuming chat gets its own board. Channels
/// with no community-submitted publications stay quiet. Returns
/// (sent, failed); `Err` only when there is nothing to do at all.
pub async fn scoreboard_round(state: &SharedState, bot: &Bot) -> Result<(usize, usize), String> {
    let chats: BTreeSet<i64> = state
        .publisher_configs
        .list_all()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|config| config.chat_id)
        .collect();
    if chats.is_empty() {
        return Err("no consuming channels are bound".to_string());
    }

    let (mut sent, mut failed) = (0, 0);
    for chat_id in chats {
        let board = match channel_board(
            chat_id,
            &state.publications,
            &state.posts,
            &state.users,
            BOARD_SIZE,
        )
        .await
        {
            Ok(board) => board,
            Err(e) => {
                tracing::error!(event = %Event::ScoreboardFailed, chat_id, error = ?e, "board computation failed");
                failed += 1;
                continue;
            }
        };
        if board.is_empty() {
            tracing::debug!(chat_id, "no community publications yet; board skipped");
            continue;
        }
        match bot.send_message(ChatId(chat_id), board_text(&board)).await {
            Ok(_) => {
                sent += 1;
                tracing::info!(
                    event = %Event::ScoreboardSent, chat_id,
                    entries = board.len(), "channel scoreboard posted"
                );
            }
            Err(e) => {
                failed += 1;
                tracing::warn!(event = %Event::ScoreboardFailed, chat_id, error = %e, "scoreboard delivery failed");
            }
        }
    }
    if let Err(e) = state.scoreboards.mark_posted(Utc::now()).await {
        tracing::error!(event = %Event::ScoreboardFailed, error = %e, "could not record scoreboard time");
    }
    Ok((sent, failed))
}

/// Minute loop: fire a round whenever the configured recurrence says so.
pub async fn run(state: SharedState, bot: Bot) -> ! {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let due = match state.scoreboards.get().await {
            Ok(settings) => settings.due(Utc::now()),
            Err(e) => {
                tracing::error!(event = %Event::ScoreboardFailed, error = %e, "settings read failed");
                false
            }
        };
        if due && let Err(reason) = scoreboard_round(&state, &bot).await {
            tracing::debug!(event = %Event::ScoreboardFailed, reason, "round skipped");
        }
    }
}
