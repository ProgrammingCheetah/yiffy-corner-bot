//! The teloxide command surface. Thin: parse + reply formatting only; every
//! decision lives in the `application` use cases.

use application::commands::{
    ban_user::{self, BanCommand},
    browse::{self, BrowseCommand, SaveCommand},
    manage_poster::{self, NewPoster, SetChannel},
    moderate::{self, ModerateCommand},
    set_user_role::{self, SetUserRole},
    start::{self, StartCommand},
    suggest::{self, SuggestCommand, SuggestOutcome},
};
use application::traits::handler_response::HandlerError;
use domain::elements::{
    cadence::PostInterval,
    post::PostId,
    tag::Tag,
    user::{Role, TelegramId},
};
use teloxide::{
    Bot,
    prelude::*,
    types::{InlineKeyboardButton, InlineKeyboardMarkup, LinkPreviewOptions, User as TgUser},
    utils::command::BotCommands,
};
use url::Url;

use crate::resolvers::BotUserResolver;
use crate::state::{PendingForward, PendingSubmission, SharedState};
use telemetry::{Event, RejectReason};

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub enum Command {
    #[command(description = "register with the bot")]
    Start,
    #[command(description = "show this help")]
    Help,
    #[command(description = "submit art by source URL")]
    Suggest(String),
    #[command(description = "moderation queue (mods)")]
    Queue,
    #[command(description = "approve a queued post (mods)")]
    Approve(String),
    #[command(description = "reject a queued post (mods)")]
    Reject(String),
    #[command(description = "soft-delete a post (mods)")]
    Delete(String),
    #[command(description = "ban a user from submitting (mods)")]
    Ban(String),
    #[command(description = "lift a submission ban (mods)")]
    Unban(String),
    #[command(description = "browse e621 by tags (mods)")]
    Browse(String),
    #[command(description = "save a browsed e621 post to the pool (mods)")]
    Save(String),
    #[command(description = "globally forbid a tag (mods)")]
    Forbidtag(String),
    #[command(description = "lift a global tag ban (mods)")]
    Unforbidtag(String),
    #[command(description = "always add a tag to e621 queries (mods)")]
    Requiretag(String),
    #[command(description = "remove a required tag (mods)")]
    Unrequiretag(String),
    #[command(description = "list global tag policies (mods)")]
    Listtags,
    #[command(description = "set a user's role: /setrole <@user|id> <moderator|user> (owner)")]
    Setrole(String),
    #[command(
        description = "create a poster: /newposter <interval-min> <tags… -forbidden…> (owner)"
    )]
    Newposter(String),
    #[command(
        description = "bind a poster to a chat: /setchannel <poster-id> <@channel|chat-id> (owner)"
    )]
    Setchannel(String),
    #[command(
        description = "replace a poster's tags: /settags <poster-id> [tags… -forbidden…] (owner)"
    )]
    Settags(String),
    #[command(description = "list posters and their bindings (owner)")]
    Posters,
}

/// Stable command label for the `command_received` event.
fn command_name(cmd: &Command) -> &'static str {
    match cmd {
        Command::Start => "start",
        Command::Help => "help",
        Command::Suggest(_) => "suggest",
        Command::Queue => "queue",
        Command::Approve(_) => "approve",
        Command::Reject(_) => "reject",
        Command::Delete(_) => "delete",
        Command::Ban(_) => "ban",
        Command::Unban(_) => "unban",
        Command::Browse(_) => "browse",
        Command::Save(_) => "save",
        Command::Forbidtag(_) => "forbidtag",
        Command::Unforbidtag(_) => "unforbidtag",
        Command::Requiretag(_) => "requiretag",
        Command::Unrequiretag(_) => "unrequiretag",
        Command::Listtags => "listtags",
        Command::Setrole(_) => "setrole",
        Command::Newposter(_) => "newposter",
        Command::Setchannel(_) => "setchannel",
        Command::Settags(_) => "settags",
        Command::Posters => "posters",
    }
}

fn sender(msg: &Message) -> Option<(&TgUser, TelegramId)> {
    let from = msg.from.as_ref()?;
    Some((from, TelegramId::from(from.id.0 as i64)))
}

fn describe(err: HandlerError) -> String {
    match err {
        HandlerError::NotAuthorized(_) | HandlerError::UnknownActor => {
            "You are not allowed to do that.".to_string()
        }
        other => other.to_string(),
    }
}

/// The moderation inline keyboard attached to review DMs.
fn review_keyboard(post_id: PostId) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([[
        InlineKeyboardButton::callback("✅ Approve", format!("mod:approve:{post_id}")),
        InlineKeyboardButton::callback("❌ Reject", format!("mod:reject:{post_id}")),
    ]])
}

pub async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    state: SharedState,
) -> ResponseResult<()> {
    let Some((from, actor)) = sender(&msg) else {
        return Ok(());
    };
    let display_name = Some(from.full_name());
    tracing::debug!(
        event = %Event::CommandReceived,
        telegram_id = actor.as_ref(),
        command = command_name(&cmd),
        "command received"
    );

    let reply = match cmd {
        Command::Start => {
            match start::handle(
                StartCommand {
                    id: actor,
                    display_name,
                },
                &state.users,
            )
            .await
            {
                Ok(()) => {
                    "Welcome to Yiffy Corner! Submit art with /suggest <source-url>.".to_string()
                }
                Err(e) => describe(e),
            }
        }
        Command::Help => Command::descriptions().to_string(),
        Command::Suggest(arg) => {
            handle_suggest(&bot, &state, actor, display_name, arg.trim()).await
        }
        Command::Queue => match moderate::queue(actor, &state.users, &state.posts).await {
            Ok(queue) if queue.is_empty() => "The moderation queue is empty.".to_string(),
            Ok(queue) => queue
                .iter()
                .map(|p| format!("#{} — {}", p.id, p.source.as_ref()))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => describe(e),
        },
        Command::Approve(arg) => moderate_reply(&state, actor, &arg, true).await,
        Command::Reject(arg) => moderate_reply(&state, actor, &arg, false).await,
        Command::Delete(arg) => match parse_post_id(&arg) {
            Some(post_id) => match moderate::delete(
                ModerateCommand { actor, post_id },
                &state.users,
                &state.posts,
            )
            .await
            {
                Ok(()) => format!("Post #{post_id} deleted."),
                Err(e) => describe(e),
            },
            None => "Usage: /delete <post-id>".to_string(),
        },
        Command::Ban(arg) => ban_reply(&bot, &state, actor, &arg, true).await,
        Command::Unban(arg) => ban_reply(&bot, &state, actor, &arg, false).await,
        Command::Browse(arg) => handle_browse(&bot, msg.chat.id, &state, actor, &arg).await,
        Command::Save(arg) => match Url::parse(arg.trim()) {
            Ok(url) => {
                match browse::save(
                    SaveCommand { actor, url },
                    &state.users,
                    &state.posts,
                    &*state.e621,
                )
                .await
                {
                    Ok(post) => format!("Saved to the pool as #{} (Accepted).", post.id),
                    Err(e) => describe(e),
                }
            }
            Err(_) => "Usage: /save <e621-url>".to_string(),
        },
        Command::Forbidtag(arg) => {
            tag_policy_reply(&state, actor, &arg, TagPolicyAction::Forbid).await
        }
        Command::Unforbidtag(arg) => {
            tag_policy_reply(&state, actor, &arg, TagPolicyAction::Unforbid).await
        }
        Command::Requiretag(arg) => {
            tag_policy_reply(&state, actor, &arg, TagPolicyAction::Require).await
        }
        Command::Unrequiretag(arg) => {
            tag_policy_reply(&state, actor, &arg, TagPolicyAction::Unrequire).await
        }
        Command::Listtags => list_tags(&state, actor).await,
        Command::Setrole(arg) => handle_setrole(&bot, &state, actor, &arg).await,
        Command::Newposter(arg) => handle_newposter(&state, actor, &arg).await,
        Command::Setchannel(arg) => handle_setchannel(&bot, &state, actor, &arg).await,
        Command::Settags(arg) => handle_settags(&state, actor, &arg).await,
        Command::Posters => handle_posters(&state, actor).await,
    };

    bot.send_message(msg.chat.id, reply)
        .link_preview_options(no_preview())
        .await?;
    Ok(())
}

/// Review DMs and command replies shouldn't unfurl every URL.
fn no_preview() -> LinkPreviewOptions {
    LinkPreviewOptions {
        is_disabled: true,
        url: None,
        prefer_small_media: false,
        prefer_large_media: false,
        show_above_text: false,
    }
}

/// The shared submission pipeline behind /suggest, the tag dialogue, and
/// channel forwards. Handles all outcomes: queueing (with review fan-out —
/// copies for forwards, text for links), tag prompting (pending state), and
/// rejections. Returns the reply for the submitter.
async fn submit(
    bot: &Bot,
    state: &SharedState,
    actor: TelegramId,
    display_name: Option<String>,
    url: Url,
    tags: Vec<Tag>,
    forward: Option<PendingForward>,
) -> String {
    use domain::elements::telegram::{TelegramCopyRef, TelegramCopyRepository as _};
    use teloxide::payloads::CopyMessageSetters as _;
    use teloxide::types::MessageId;

    let submitter_name = display_name.clone().unwrap_or_else(|| "a user".to_string());
    let outcome = suggest::handle(
        SuggestCommand {
            submitter: actor,
            display_name,
            url: url.clone(),
            tags,
        },
        &state.users,
        &state.posts,
        &*state.e621,
        &state.forbidden,
    )
    .await;
    match outcome {
        Ok(SuggestOutcome::TagsNeeded) => {
            state
                .pending
                .lock()
                .await
                .insert(*actor.as_ref(), PendingSubmission { url, forward });
            "Almost there! Reply with the tags that describe this post, separated by \
             spaces — species, character, artist, anything relevant.\n\
             Example: `wolf male solo digital_art`"
                .to_string()
        }
        Ok(SuggestOutcome::Queued { post, reviewers }) => {
            if let Some(fwd) = &forward {
                if let Err(e) = state
                    .telegram_copies
                    .upsert(TelegramCopyRef {
                        source_url: post.source.as_ref().as_str().to_string(),
                        origin_chat_id: fwd.origin_chat_id,
                        origin_message_id: fwd.origin_message_id,
                        channel_username: fwd.channel_username.clone(),
                    })
                    .await
                {
                    tracing::error!(event = %Event::CopyRefStoreFailed, post_id = %post.id, error = %e, "copy-ref store failed");
                } else {
                    tracing::info!(
                        event = %Event::CopyRefStored, post_id = %post.id,
                        channel = fwd.channel_username, origin_message_id = fwd.origin_message_id,
                        "copy coordinates stored"
                    );
                }
            }

            let tag_line = post
                .tags
                .iter()
                .take(12)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            let origin_line = match &forward {
                Some(fwd) => format!("Forwarded from channel: @{}", fwd.channel_username),
                None => post.source.as_ref().to_string(),
            };
            let text = format!(
                "New submission #{}\n{origin_line}\nTags: {tag_line}\nSubmitted by {submitter_name}",
                post.id
            );
            for reviewer in &reviewers {
                let reviewer_chat = ChatId(*reviewer.telegram_id.as_ref());
                let sent = match &forward {
                    Some(fwd) => bot
                        .copy_message(
                            reviewer_chat,
                            ChatId(fwd.origin_chat_id),
                            MessageId(fwd.origin_message_id),
                        )
                        .caption(text.clone())
                        .reply_markup(review_keyboard(post.id))
                        .await
                        .map(|_| ()),
                    None => bot
                        .send_message(reviewer_chat, text.clone())
                        .reply_markup(review_keyboard(post.id))
                        .await
                        .map(|_| ()),
                };
                match sent {
                    Ok(()) => tracing::debug!(
                        event = %Event::ReviewDmSent, post_id = %post.id,
                        reviewer = %reviewer.id, copied = forward.is_some(), "review DM sent"
                    ),
                    Err(e) => tracing::warn!(
                        event = %Event::ReviewDmFailed, post_id = %post.id,
                        reviewer = %reviewer.id, error = %e, "review DM failed"
                    ),
                }
            }
            match &forward {
                Some(fwd) => format!(
                    "Submission #{} is in the moderation queue — it will be posted as a copy \
                     credited to @{} once approved!",
                    post.id, fwd.channel_username
                ),
                None => format!(
                    "Submission #{} is in the moderation queue — you'll see it posted once approved!",
                    post.id
                ),
            }
        }
        Ok(SuggestOutcome::AutoBanned { .. }) => {
            "This post contains content that is not allowed here.".to_string()
        }
        Err(e) => describe(e),
    }
}

async fn handle_suggest(
    bot: &Bot,
    state: &SharedState,
    actor: TelegramId,
    display_name: Option<String>,
    arg: &str,
) -> String {
    let mut parts = arg.split_whitespace();
    let Some(url) = parts.next().and_then(|raw| Url::parse(raw).ok()) else {
        return "Usage: /suggest <source-url> [tags…] — e621, FurAffinity, Twitter/X, BlueSky, \
                DeviantArt and t.me links are accepted. Non-e621 sources need tags \
                (I'll ask if you leave them off)."
            .to_string();
    };
    let tags: Vec<Tag> = parts.map(Tag::from).collect();
    submit(bot, state, actor, display_name, url, tags, None).await
}

/// Completes a pending submission: the submitter's next plain-text message
/// after a tag prompt carries the tags.
pub async fn handle_pending_tags(bot: Bot, msg: Message, state: SharedState) -> ResponseResult<()> {
    let Some((from, actor)) = sender(&msg) else {
        return Ok(());
    };
    let Some(text) = msg.text() else {
        return Ok(());
    };
    let Some(pending) = state.pending.lock().await.remove(actor.as_ref()) else {
        return Ok(()); // no dialogue in flight — stay silent
    };
    let tags: Vec<Tag> = text.split_whitespace().map(Tag::from).collect();
    if tags.is_empty() {
        state.pending.lock().await.insert(*actor.as_ref(), pending);
        bot.send_message(
            msg.chat.id,
            "I need at least one tag — try `wolf male solo`.",
        )
        .await?;
        return Ok(());
    }
    let reply = submit(
        &bot,
        &state,
        actor,
        Some(from.full_name()),
        pending.url,
        tags,
        pending.forward,
    )
    .await;
    bot.send_message(msg.chat.id, reply)
        .link_preview_options(no_preview())
        .await?;
    Ok(())
}

fn parse_post_id(arg: &str) -> Option<PostId> {
    arg.trim()
        .trim_start_matches('#')
        .parse::<u64>()
        .ok()
        .map(PostId::from)
}

async fn moderate_reply(
    state: &SharedState,
    actor: TelegramId,
    arg: &str,
    approve: bool,
) -> String {
    let Some(post_id) = parse_post_id(arg) else {
        return "Usage: /approve <post-id> (or /reject)".to_string();
    };
    let cmd = ModerateCommand { actor, post_id };
    let result = if approve {
        moderate::approve(cmd, &state.users, &state.posts).await
    } else {
        moderate::reject(cmd, &state.users, &state.posts).await
    };
    match result {
        Ok(post) => format!("Post #{} is now {:?}.", post.id, post.status),
        Err(e) => describe(e),
    }
}

async fn ban_reply(
    bot: &Bot,
    state: &SharedState,
    actor: TelegramId,
    arg: &str,
    banned: bool,
) -> String {
    let resolver = BotUserResolver { bot: bot.clone() };
    let target = match resolve_target(&resolver, arg).await {
        Ok(Some(id)) => id,
        Ok(None) => return format!("I can't find {arg}. Use their numeric Telegram ID."),
        Err(e) => return e,
    };
    match ban_user::handle(
        BanCommand {
            actor,
            target,
            banned,
        },
        &state.users,
    )
    .await
    {
        Ok(()) if banned => "Banned from submitting.".to_string(),
        Ok(()) => "Ban lifted.".to_string(),
        Err(e) => describe(e),
    }
}

async fn resolve_target(
    resolver: &BotUserResolver,
    arg: &str,
) -> Result<Option<TelegramId>, String> {
    use domain::elements::telegram::TelegramUserResolver as _;
    resolver
        .resolve_username(arg.trim())
        .await
        .map_err(|e| e.to_string())
}

/// The artist's preferred off-site source, mirroring the legacy priority
/// list; falls back to the first declared source.
fn preferred_artist_source(sources: &[String]) -> Option<Url> {
    const PREFERRED_HOSTS: [&str; 6] = [
        "twitter.com",
        "x.com",
        "furaffinity.net",
        "tumblr.com",
        "deviantart.com",
        "pixiv.net",
    ];
    let parsed: Vec<Url> = sources.iter().filter_map(|s| Url::parse(s).ok()).collect();
    for host in PREFERRED_HOSTS {
        if let Some(url) = parsed
            .iter()
            .find(|u| u.host_str().is_some_and(|h| h.ends_with(host)))
        {
            return Some(url.clone());
        }
    }
    parsed.first().cloned()
}

/// The legacy 4-button browse keyboard: Send saves to the pool, the two URL
/// buttons open e621 / the artist's source, Erase dismisses the preview.
fn browse_keyboard(
    e621_id: u64,
    e621_url: &Url,
    artist_sources: &[String],
) -> InlineKeyboardMarkup {
    let src = preferred_artist_source(artist_sources).unwrap_or_else(|| e621_url.clone());
    InlineKeyboardMarkup::new([
        vec![InlineKeyboardButton::callback(
            "Send",
            format!("browse:send:{e621_id}"),
        )],
        vec![
            InlineKeyboardButton::url("Check e621 Src", e621_url.clone()),
            InlineKeyboardButton::url("Check src", src),
        ],
        vec![InlineKeyboardButton::callback("Erase", "browse:erase")],
    ])
}

async fn handle_browse(
    bot: &Bot,
    chat: ChatId,
    state: &SharedState,
    actor: TelegramId,
    arg: &str,
) -> String {
    use teloxide::types::InputFile;

    let tags: Vec<Tag> = arg.split_whitespace().map(Tag::from).collect();
    match browse::search(
        BrowseCommand {
            actor,
            tags,
            page: 1,
        },
        &state.users,
        &*state.e621,
        &state.forbidden,
        &state.required,
    )
    .await
    {
        Ok(results) if results.is_empty() => "No matching e621 posts.".to_string(),
        Ok(results) => {
            // Like the legacy bot: each result is its own photo with the
            // Send / sources / Erase keyboard.
            let mut sent = 0usize;
            for metadata in results.iter().take(5) {
                let e621_url = metadata.source.as_ref();
                let Some(e621_id) = e621_url
                    .path_segments()
                    .and_then(|mut s| s.nth(1))
                    .and_then(|id| id.parse::<u64>().ok())
                else {
                    continue;
                };
                match bot
                    .send_photo(chat, InputFile::url(metadata.preview_url.clone()))
                    .reply_markup(browse_keyboard(e621_id, e621_url, &metadata.artist_sources))
                    .await
                {
                    Ok(_) => sent += 1,
                    Err(e) => tracing::warn!(
                        event = %Event::BrowseAlbumFailed, source = %e621_url, error = %e,
                        "browse preview send failed"
                    ),
                }
            }
            if sent == 0 {
                "Couldn't send any previews — check the logs.".to_string()
            } else {
                format!("{sent} results — Send saves to the pool, Erase dismisses.")
            }
        }
        Err(e) => describe(e),
    }
}

enum TagPolicyAction {
    Forbid,
    Unforbid,
    Require,
    Unrequire,
}

async fn tag_policy_reply(
    state: &SharedState,
    actor: TelegramId,
    arg: &str,
    action: TagPolicyAction,
) -> String {
    use application::commands::auth::require_role;
    use domain::elements::tag_policy::{ForbiddenTagRepository, RequiredTagRepository};

    if let Err(e) = require_role(&state.users, actor, Role::Moderator).await {
        return describe(e);
    }
    let tag = arg.trim();
    if tag.is_empty() || tag.contains(char::is_whitespace) {
        return "Give exactly one tag.".to_string();
    }
    let tag = Tag::from(tag);
    tracing::info!(
        event = %Event::TagPolicyChanged,
        telegram_id = actor.as_ref(),
        action = match action {
            TagPolicyAction::Forbid => "forbid",
            TagPolicyAction::Unforbid => "unforbid",
            TagPolicyAction::Require => "require",
            TagPolicyAction::Unrequire => "unrequire",
        },
        tag = %tag,
        "tag policy changed"
    );
    let result = match action {
        TagPolicyAction::Forbid => state
            .forbidden
            .add(tag.clone())
            .await
            .map_err(|e| e.to_string()),
        TagPolicyAction::Unforbid => state
            .forbidden
            .remove(&tag)
            .await
            .map_err(|e| e.to_string()),
        TagPolicyAction::Require => state
            .required
            .add(tag.clone())
            .await
            .map_err(|e| e.to_string()),
        TagPolicyAction::Unrequire => state.required.remove(&tag).await.map_err(|e| e.to_string()),
    };
    match result {
        Ok(()) => format!("Tag policy updated: {tag}."),
        Err(e) => e,
    }
}

async fn list_tags(state: &SharedState, actor: TelegramId) -> String {
    use application::commands::auth::require_role;
    use domain::elements::tag_policy::{ForbiddenTagRepository, RequiredTagRepository};

    if let Err(e) = require_role(&state.users, actor, Role::Moderator).await {
        return describe(e);
    }
    let forbidden = state
        .forbidden
        .list_all()
        .await
        .unwrap_or_default()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let required = state
        .required
        .list_all()
        .await
        .unwrap_or_default()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("FORBIDDEN: {forbidden}\nREQUIRED: {required}")
}

async fn handle_setrole(bot: &Bot, state: &SharedState, actor: TelegramId, arg: &str) -> String {
    let mut parts = arg.split_whitespace();
    let (Some(target), Some(role_raw), None) = (parts.next(), parts.next(), parts.next()) else {
        return "Usage: /setrole <@user|id> <moderator|user>".to_string();
    };
    let new_role = match role_raw.to_ascii_lowercase().as_str() {
        "moderator" | "mod" => Role::Moderator,
        "user" => Role::User,
        _ => return "Role must be `moderator` or `user`.".to_string(),
    };
    let resolver = BotUserResolver { bot: bot.clone() };
    match set_user_role::handle(
        SetUserRole {
            actor,
            target_username: target.trim_start_matches('@').to_string(),
            new_role,
        },
        &state.users,
        &resolver,
    )
    .await
    {
        Ok(user) => format!("{} is now {}.", target, user.role),
        Err(e) => describe(e),
    }
}

/// Split "wolf male -gore" into (subscribed, forbidden) tag lists.
fn parse_tag_lists<'a>(parts: impl Iterator<Item = &'a str>) -> (Vec<Tag>, Vec<Tag>) {
    let mut subscribed = Vec::new();
    let mut forbidden = Vec::new();
    for raw in parts {
        match raw.strip_prefix('-') {
            Some(tag) => forbidden.push(Tag::from(tag)),
            None => subscribed.push(Tag::from(raw)),
        }
    }
    (subscribed, forbidden)
}

async fn handle_settags(state: &SharedState, actor: TelegramId, arg: &str) -> String {
    let mut parts = arg.split_whitespace();
    let Some(poster_id) = parts
        .next()
        .and_then(|v| v.trim_start_matches('#').parse::<u64>().ok())
        .map(domain::elements::poster::PosterId::from)
    else {
        return "Usage: /settags <poster-id> [tags… -forbidden…]\n\
                No tags = post anything (subscription filter removed)."
            .to_string();
    };
    let (subscribed, forbidden) = parse_tag_lists(parts);
    match manage_poster::set_tags(
        manage_poster::SetTags {
            actor,
            poster_id,
            subscribed_tags: subscribed,
            forbidden_tags: forbidden,
        },
        &state.users,
        &state.posters,
    )
    .await
    {
        Ok(poster) if poster.subscribed_tags.is_empty() => format!(
            "Poster #{} now posts ANYTHING (no subscription filter). Restart the bot to apply.",
            poster.id
        ),
        Ok(poster) => format!(
            "Poster #{} now subscribes to [{}] minus [{}]. Restart the bot to apply.",
            poster.id,
            poster
                .subscribed_tags
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
            poster
                .forbidden_tags
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Err(e) => describe(e),
    }
}

async fn handle_newposter(state: &SharedState, actor: TelegramId, arg: &str) -> String {
    let mut parts = arg.split_whitespace();
    let Some(interval) = parts.next().and_then(|v| v.parse::<u8>().ok()) else {
        return "Usage: /newposter <interval-minutes> <tags… -forbidden…>\n\
                Interval must divide 60 (1,2,3,4,5,6,10,12,15,20,30,60)."
            .to_string();
    };
    let interval = match PostInterval::new(interval) {
        Ok(i) => i,
        Err(e) => return e.to_string(),
    };
    let (subscribed, forbidden) = parse_tag_lists(parts);
    match manage_poster::new_poster(
        NewPoster {
            actor,
            subscribed_tags: subscribed,
            forbidden_tags: forbidden,
            interval,
        },
        &state.users,
        &state.posters,
    )
    .await
    {
        Ok(poster) => format!(
            "Poster #{} created. Bind it with /setchannel {} <@channel|chat-id>.",
            poster.id, poster.id
        ),
        Err(e) => describe(e),
    }
}

async fn handle_setchannel(bot: &Bot, state: &SharedState, actor: TelegramId, arg: &str) -> String {
    let mut parts = arg.split_whitespace();
    let (Some(poster_raw), Some(chat_raw)) = (parts.next(), parts.next()) else {
        return "Usage: /setchannel <poster-id> <@channel|chat-id>".to_string();
    };
    let Some(poster_id) = poster_raw
        .trim_start_matches('#')
        .parse::<u64>()
        .ok()
        .map(domain::elements::poster::PosterId::from)
    else {
        return "Poster id must be numeric.".to_string();
    };
    let chat_id = if let Ok(id) = chat_raw.parse::<i64>() {
        id
    } else {
        let resolver = BotUserResolver { bot: bot.clone() };
        match resolve_target(&resolver, chat_raw).await {
            Ok(Some(id)) => *id.as_ref(),
            Ok(None) => return format!("Can't resolve {chat_raw} — is the bot in that channel?"),
            Err(e) => return e,
        }
    };
    match manage_poster::set_channel(
        SetChannel {
            actor,
            poster_id,
            chat_id,
            // MVP: every Poster publishes with the main bot token.
            token_path: state.config.token_path(),
        },
        &state.users,
        &state.posters,
        &state.publisher_configs,
    )
    .await
    {
        Ok(()) => {
            format!("Poster #{poster_id} now publishes to {chat_id}. Restart the bot to activate.")
        }
        Err(e) => describe(e),
    }
}

async fn handle_posters(state: &SharedState, actor: TelegramId) -> String {
    use application::commands::auth::require_role;
    use domain::elements::poster::PosterRepository;
    use domain::elements::publisher_config::PublisherConfigRepository;

    if let Err(e) = require_role(&state.users, actor, Role::Owner).await {
        return describe(e);
    }
    let posters = match state.posters.list_all().await {
        Ok(p) => p,
        Err(e) => return e.to_string(),
    };
    if posters.is_empty() {
        return "No posters yet. Create one with /newposter.".to_string();
    }
    let mut lines = Vec::new();
    for poster in posters {
        let binding = match state.publisher_configs.find_by_poster(poster.id).await {
            Ok(Some(config)) => format!("→ chat {}", config.chat_id),
            _ => "→ UNBOUND (use /setchannel)".to_string(),
        };
        lines.push(format!(
            "#{} every {}min, tags [{}] minus [{}] {}",
            poster.id,
            poster.time_interval.as_ref(),
            poster
                .subscribed_tags
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
            poster
                .forbidden_tags
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
            binding
        ));
    }
    lines.join("\n")
}

/// A message forwarded from a channel into the bot's private chat is a
/// submission. Per design (2026-07-04): the bot never re-*forwards* — it
/// *copies* the content and tags the origin at the bottom with
/// "Forwarded from channel: @<channel>". Reviewers see exactly that copy.
pub async fn handle_channel_forward(
    bot: Bot,
    msg: Message,
    state: SharedState,
) -> ResponseResult<()> {
    use teloxide::types::MessageOrigin;

    let Some((from, actor)) = sender(&msg) else {
        return Ok(());
    };
    let Some(MessageOrigin::Channel {
        chat, message_id, ..
    }) = msg.forward_origin()
    else {
        return Ok(());
    };
    let Some(channel) = chat.username() else {
        tracing::info!(
            event = %Event::ForwardRejected, reason = %RejectReason::PrivateChannel,
            telegram_id = actor.as_ref(), "forward from channel without @username"
        );
        bot.send_message(
            msg.chat.id,
            "I can only take submissions forwarded from public channels \
             (the channel needs an @username).",
        )
        .await?;
        return Ok(());
    };
    let Ok(url) = Url::parse(&format!("https://t.me/{channel}/{}", message_id.0)) else {
        bot.send_message(msg.chat.id, "That forward has no usable origin link.")
            .await?;
        return Ok(());
    };

    // t.me sources always need tags, so this lands in the tag dialogue
    // (after the duplicate/ban checks inside the submission pipeline).
    let reply = submit(
        &bot,
        &state,
        actor,
        Some(from.full_name()),
        url,
        Vec::new(),
        Some(PendingForward {
            origin_chat_id: msg.chat.id.0,
            origin_message_id: msg.id.0,
            channel_username: channel.to_string(),
        }),
    )
    .await;
    bot.send_message(msg.chat.id, reply)
        .link_preview_options(no_preview())
        .await?;
    Ok(())
}

/// Inline Approve/Reject buttons on review DMs.
pub async fn handle_callback(
    bot: Bot,
    query: CallbackQuery,
    state: SharedState,
) -> ResponseResult<()> {
    let actor = TelegramId::from(query.from.id.0 as i64);
    tracing::debug!(
        event = %Event::CallbackReceived,
        telegram_id = actor.as_ref(),
        data = query.data.as_deref().unwrap_or(""),
        "callback received"
    );
    let Some(data) = query.data.as_deref() else {
        bot.answer_callback_query(query.id).await?;
        return Ok(());
    };
    match data.split(':').collect::<Vec<_>>()[..] {
        ["mod", verb @ ("approve" | "reject"), id] => {
            let outcome = match parse_post_id(id) {
                Some(post_id) => {
                    let cmd = ModerateCommand { actor, post_id };
                    let result = if verb == "approve" {
                        moderate::approve(cmd, &state.users, &state.posts).await
                    } else {
                        moderate::reject(cmd, &state.users, &state.posts).await
                    };
                    match result {
                        Ok(post) => format!("Post #{} → {:?}", post.id, post.status),
                        Err(e) => describe(e),
                    }
                }
                None => "Malformed callback.".to_string(),
            };
            bot.answer_callback_query(query.id.clone()).await?;
            // Reflect the decision on the DM itself so the buttons disappear.
            if let Some(message) = query.message.as_ref() {
                let text = format!(
                    "{}\n\n{outcome}",
                    message
                        .regular_message()
                        .and_then(|m| m.text())
                        .unwrap_or("")
                );
                bot.edit_message_text(message.chat().id, message.id(), text)
                    .await?;
            }
        }
        // Legacy browse buttons: any press dismisses the preview message;
        // Send additionally saves the post into the pool.
        ["browse", "erase"] => {
            bot.answer_callback_query(query.id.clone()).await?;
            if let Some(message) = query.message.as_ref() {
                let _ = bot.delete_message(message.chat().id, message.id()).await;
            }
        }
        ["browse", "send", id] => {
            use teloxide::payloads::AnswerCallbackQuerySetters as _;

            let toast = match id
                .parse::<u64>()
                .ok()
                .and_then(|id| Url::parse(&format!("https://e621.net/posts/{id}")).ok())
            {
                Some(url) => {
                    match browse::save(
                        SaveCommand { actor, url },
                        &state.users,
                        &state.posts,
                        &*state.e621,
                    )
                    .await
                    {
                        Ok(post) => format!("Saved to the pool as #{}.", post.id),
                        Err(e) => describe(e),
                    }
                }
                None => "Malformed callback.".to_string(),
            };
            bot.answer_callback_query(query.id.clone())
                .text(toast)
                .await?;
            if let Some(message) = query.message.as_ref() {
                let _ = bot.delete_message(message.chat().id, message.id()).await;
            }
        }
        _ => {
            bot.answer_callback_query(query.id.clone()).await?;
        }
    }
    Ok(())
}
