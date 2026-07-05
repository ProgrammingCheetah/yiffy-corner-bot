//! Periodic housekeeping loop: runs the dead-media sweep and DMs the Owner
//! a digest when unconsumed feed entries have lost their upstream media.

use std::time::Duration;

use application::actors::housekeeper::run_sweep;
use teloxide::{Bot, prelude::Requester, types::ChatId};

use crate::state::SharedState;

/// First sweep shortly after boot (let the stack settle), then every 6 h.
const FIRST_SWEEP_AFTER: Duration = Duration::from_secs(15 * 60);
const SWEEP_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// Between entries: throttles the unpaced backends (FixUp, raw image
/// downloads) and keeps the sweep from monopolizing the shared e621 pacer.
const SWEEP_PACE: Duration = Duration::from_secs(2);

pub async fn run(state: SharedState, bot: Bot) {
    tokio::time::sleep(FIRST_SWEEP_AFTER).await;
    let mut ticker = tokio::time::interval(SWEEP_EVERY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let outcome = match run_sweep(
            &state.posts,
            &state.posters,
            &*state.resolver,
            &*state.hasher,
            SWEEP_PACE,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::warn!(error = %e, "dead-media sweep failed; retrying next cycle");
                continue;
            }
        };
        if outcome.dead.is_empty() {
            continue;
        }
        let mut lines = vec![format!(
            "🧹 Dead-media sweep: {} of {} pending feed entries lost their upstream media \
             and were shelved (media_gone):",
            outcome.dead.len(),
            outcome.scanned
        )];
        for entry in &outcome.dead {
            lines.push(format!("#{} — {}", entry.post_id, entry.source));
        }
        if outcome.revived > 0 {
            lines.push(format!(
                "(Also revived {} previously-shelved entr{} whose source came back.)",
                outcome.revived,
                if outcome.revived == 1 { "y" } else { "ies" }
            ));
        }
        lines.push(
            "Every poster skips shelved entries; they revive automatically if the source \
             returns, or /delete for a permanent takedown."
                .into(),
        );
        if let Err(e) = bot
            .send_message(ChatId(*state.config.owner_id.as_ref()), lines.join("\n"))
            .await
        {
            tracing::warn!(error = %e, "sweep digest DM failed");
        }
    }
}
