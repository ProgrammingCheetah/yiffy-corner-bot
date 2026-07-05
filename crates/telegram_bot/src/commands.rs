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
use crate::state::{BrowseSession, PendingForward, PendingSubmission, SharedState};
use telemetry::{Event, RejectReason};

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub enum Command {
    #[command(description = "register with the bot")]
    Start(String),
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
    #[command(description = "full data for a post: /postinfo <post-id> (mods)")]
    Postinfo(String),
    #[command(description = "ban a user from submitting (mods)")]
    Ban(String),
    #[command(description = "lift a submission ban (mods)")]
    Unban(String),
    #[command(description = "browse e621 by tags (mods)")]
    Browse(String),
    #[command(description = "add any source straight to the feed: /save <url> [tags…] (mods)")]
    Save(String),
    #[command(description = "globally forbid a tag (mods)")]
    Forbidtag(String),
    #[command(description = "lift a global tag ban (mods)")]
    Unforbidtag(String),
    #[command(description = "always add a tag to e621 queries (mods)")]
    Requiretag(String),
    #[command(description = "remove a required tag (mods)")]
    Unrequiretag(String),
    #[command(description = "spoiler media carrying a tag (mods)")]
    Spoilertag(String),
    #[command(description = "stop spoilering a tag (mods)")]
    Unspoilertag(String),
    #[command(description = "list global tag policies (mods)")]
    Listtags,
    #[command(description = "set a user's role: /setrole <@user|id> <moderator|user> (owner)")]
    Setrole(String),
    #[command(
        description = "create a poster: /newposter <interval-min> <@channel|chat-id> [tags… -forbidden…] (owner)"
    )]
    Newposter(String),
    #[command(
        description = "bind a poster to a chat: /setchannel <poster-id> <@channel|chat-id> (owner)"
    )]
    Setchannel(String),
    #[command(
        description = "replace a poster's tags: /settags <poster|@channel|chat-id> [tags… -forbidden…] (owner)"
    )]
    Settags(String),
    #[command(
        description = "add tags without rewriting: /addtags <poster|@channel|chat-id> [tags… -forbidden…] (owner)"
    )]
    Addtags(String),
    #[command(
        description = "remove tags without rewriting: /deltags <poster|@channel|chat-id> [tags… -forbidden…] (owner)"
    )]
    Deltags(String),
    #[command(
        description = "change a poster's cadence: /setinterval <poster|@channel|chat-id> <minutes> (owner)"
    )]
    Setinterval(String),
    #[command(
        description = "conditional tag rules: /setrules <poster|@channel|chat-id> [if…]->[then…] … (owner)"
    )]
    Setrules(String),
    #[command(
        description = "append rules without rewriting: /addrules <poster|@channel|chat-id> [if…]->[then…] … (owner)"
    )]
    Addrules(String),
    #[command(
        description = "delete rules by number from /posters: /delrules <poster|@channel|chat-id> <n…> (owner)"
    )]
    Delrules(String),
    #[command(description = "delete a poster: /delposter <poster-id> (owner)")]
    Delposter(String),
    #[command(description = "list posters and their bindings (owner)")]
    Posters,
    #[command(description = "channel directory broadcasts: /announcements <hours|now|off> (owner)")]
    Announcements(String),
    #[command(
        description = "pin a channel atop the directory: /spotlight <@channel|id|off> (owner)"
    )]
    Spotlight(String),
    #[command(
        description = "stop delivering announcements to a chat (it stays listed): /announcemute <@channel|id> (owner)"
    )]
    Announcemute(String),
    #[command(description = "resume announcement delivery: /announceunmute <@channel|id> (owner)")]
    Announceunmute(String),
}

/// Stable command label for the `command_received` event.
fn command_name(cmd: &Command) -> &'static str {
    match cmd {
        Command::Start(_) => "start",
        Command::Help => "help",
        Command::Suggest(_) => "suggest",
        Command::Queue => "queue",
        Command::Approve(_) => "approve",
        Command::Reject(_) => "reject",
        Command::Delete(_) => "delete",
        Command::Postinfo(_) => "postinfo",
        Command::Ban(_) => "ban",
        Command::Unban(_) => "unban",
        Command::Browse(_) => "browse",
        Command::Save(_) => "save",
        Command::Forbidtag(_) => "forbidtag",
        Command::Unforbidtag(_) => "unforbidtag",
        Command::Requiretag(_) => "requiretag",
        Command::Unrequiretag(_) => "unrequiretag",
        Command::Spoilertag(_) => "spoilertag",
        Command::Unspoilertag(_) => "unspoilertag",
        Command::Listtags => "listtags",
        Command::Setrole(_) => "setrole",
        Command::Newposter(_) => "newposter",
        Command::Setchannel(_) => "setchannel",
        Command::Settags(_) => "settags",
        Command::Addtags(_) => "addtags",
        Command::Deltags(_) => "deltags",
        Command::Setinterval(_) => "setinterval",
        Command::Setrules(_) => "setrules",
        Command::Addrules(_) => "addrules",
        Command::Delrules(_) => "delrules",
        Command::Delposter(_) => "delposter",
        Command::Posters => "posters",
        Command::Announcements(_) => "announcements",
        Command::Spotlight(_) => "spotlight",
        Command::Announcemute(_) => "announcemute",
        Command::Announceunmute(_) => "announceunmute",
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

/// File a viewer report and fan the moderator DM out. Shared by the
/// caption deep link (`/start report_<id>`) and the legacy inline button.
async fn file_report(
    bot: &Bot,
    state: &SharedState,
    reporter: TelegramId,
    post_id: PostId,
) -> String {
    use application::commands::report::{self, ReportOutcome};

    match report::report(
        reporter,
        post_id,
        &state.posts,
        &state.reports,
        &state.users,
    )
    .await
    {
        Ok(ReportOutcome::Duplicate) => "You already reported this post.".to_string(),
        Ok(ReportOutcome::New {
            post,
            reviewers,
            total_reports,
        }) => {
            let text = format!(
                "⚠️ Post #{} was reported ({total_reports} report(s))\n{}",
                post.id,
                post.source.as_ref()
            );
            let keyboard = InlineKeyboardMarkup::new([[
                InlineKeyboardButton::callback("🗑 Take down", format!("repmod:take:{}", post.id)),
                InlineKeyboardButton::callback("✅ Dismiss", format!("repmod:dismiss:{}", post.id)),
            ]]);
            for reviewer in &reviewers {
                let chat = ChatId(*reviewer.telegram_id.as_ref());
                if let Err(e) = bot
                    .send_message(chat, text.clone())
                    .reply_markup(keyboard.clone())
                    .await
                {
                    tracing::warn!(
                        event = %Event::ReportNotifyFailed, post_id = %post.id,
                        reviewer = %reviewer.id, error = %e, "report DM failed"
                    );
                }
            }
            "Thank you — the moderators have been notified.".to_string()
        }
        Err(e) => describe(e),
    }
}

/// Finish a moderation dialogue with the moderator's reply text.
async fn complete_moderation_dialogue(
    bot: &Bot,
    state: &SharedState,
    actor: TelegramId,
    dialogue: crate::state::ModerationDialogue,
    text: &str,
) -> String {
    use crate::state::ModerationDialogue;
    use domain::elements::user::UserRepository as _;

    match dialogue {
        ModerationDialogue::RejectReason(post_id) => {
            let reason = text.trim();
            if reason.is_empty() {
                return "Empty reason — post left untouched. Press the button again to retry."
                    .to_string();
            }
            match moderate::reject(
                ModerateCommand { actor, post_id },
                &state.users,
                &state.posts,
            )
            .await
            {
                Err(e) => describe(e),
                Ok(post) => {
                    // Relay the reason to the submitter.
                    let notified = match post.submitted_by {
                        None => false,
                        Some(user_id) => match state.users.find_by_id(user_id).await {
                            Ok(Some(user)) => bot
                                .send_message(
                                    ChatId(*user.telegram_id.as_ref()),
                                    format!(
                                        "Your submission #{post_id} was rejected by the \
                                         moderators.\nReason: {reason}"
                                    ),
                                )
                                .await
                                .is_ok(),
                            _ => false,
                        },
                    };
                    if notified {
                        tracing::info!(
                            event = %Event::SubmitterNotified, post_id = %post_id,
                            "rejection reason relayed to submitter"
                        );
                        format!("Post #{post_id} rejected — the submitter was told why.")
                    } else {
                        format!(
                            "Post #{post_id} rejected. (Couldn't DM the submitter — they may \
                             not have a chat open with the bot.)"
                        )
                    }
                }
            }
        }
        ModerationDialogue::ExtraTags(post_id) => {
            let extra: Vec<Tag> = text.split_whitespace().map(Tag::from).collect();
            if extra.is_empty() {
                return "No tags given — post left untouched. Press the button again to retry."
                    .to_string();
            }
            let requested = extra.len();
            match moderate::approve_with_extra_tags(
                ModerateCommand { actor, post_id },
                extra,
                &state.users,
                &state.posts,
            )
            .await
            {
                Err(e) => describe(e),
                Ok(post) => {
                    notify_submitter_approved(bot, state, &post).await;
                    format!(
                        "Post #{post_id} accepted into the feed with {} tags \
                         ({requested} supplied, duplicates ignored).",
                        post.tags.len()
                    )
                }
            }
        }
    }
}

/// Append the outcome to a review/report DM, media-aware: text messages
/// get edit_message_text, media messages (photo/video reviews) get
/// edit_message_caption. Never propagates — a cosmetic edit failing must
/// not crash the callback handler.
async fn reflect_outcome_on_dm(
    bot: &Bot,
    message: &teloxide::types::MaybeInaccessibleMessage,
    outcome: &str,
) {
    use teloxide::payloads::EditMessageCaptionSetters as _;

    let chat = message.chat().id;
    let id = message.id();
    let regular = message.regular_message();
    let result = if let Some(text) = regular.and_then(|m| m.text()) {
        bot.edit_message_text(chat, id, format!("{text}\n\n{outcome}"))
            .await
            .map(|_| ())
    } else {
        let caption = regular.and_then(|m| m.caption()).unwrap_or("");
        bot.edit_message_caption(chat, id)
            .caption(format!("{caption}\n\n{outcome}"))
            .await
            .map(|_| ())
    };
    if let Err(e) = result {
        tracing::debug!(error = %e, "review DM outcome edit failed (cosmetic)");
    }
}

/// The moderation inline keyboard attached to review DMs.
fn review_keyboard(post_id: PostId) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([
        vec![
            InlineKeyboardButton::callback("✅ Approve", format!("mod:approve:{post_id}")),
            InlineKeyboardButton::callback("❌ Reject", format!("mod:reject:{post_id}")),
        ],
        vec![
            InlineKeyboardButton::callback(
                "🏷 Accept with more tags",
                format!("mod:addtags:{post_id}"),
            ),
            InlineKeyboardButton::callback(
                "📝 Reject with reason",
                format!("mod:reason:{post_id}"),
            ),
        ],
    ])
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
        Command::Start(payload) => {
            let registration = start::handle(
                StartCommand {
                    id: actor,
                    display_name,
                },
                &state.users,
            )
            .await;
            // Deep-link payloads: `t.me/<bot>?start=report_<id>` arrives as
            // `/start report_<id>` — the buttonless Report path.
            if let Some(raw_id) = payload.trim().strip_prefix("report_") {
                match parse_post_id(raw_id) {
                    Some(post_id) => file_report(&bot, &state, actor, post_id).await,
                    None => "That report link is malformed.".to_string(),
                }
            } else {
                match registration {
                    Ok(()) => "Welcome to Yiffy Corner! Submit art with /suggest <source-url>."
                        .to_string(),
                    Err(e) => describe(e),
                }
            }
        }
        Command::Help => Command::descriptions().to_string(),
        Command::Suggest(arg) => {
            handle_suggest(&bot, &state, Submitter::from(from), arg.trim()).await
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
        Command::Approve(arg) => moderate_reply(&bot, &state, actor, &arg, true).await,
        Command::Reject(arg) => moderate_reply(&bot, &state, actor, &arg, false).await,
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
        Command::Postinfo(arg) => handle_postinfo(&state, actor, &arg).await,
        Command::Ban(arg) => ban_reply(&bot, &state, actor, &arg, true).await,
        Command::Unban(arg) => ban_reply(&bot, &state, actor, &arg, false).await,
        Command::Browse(arg) => handle_browse(&bot, msg.chat.id, &state, actor, &arg).await,
        Command::Save(arg) => handle_save(&state, actor, &arg).await,
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
        Command::Spoilertag(arg) => {
            tag_policy_reply(&state, actor, &arg, TagPolicyAction::Spoiler).await
        }
        Command::Unspoilertag(arg) => {
            tag_policy_reply(&state, actor, &arg, TagPolicyAction::Unspoiler).await
        }
        Command::Listtags => list_tags(&state, actor).await,
        Command::Setrole(arg) => handle_setrole(&bot, &state, actor, &arg).await,
        Command::Newposter(arg) => handle_newposter(&bot, &state, actor, &arg).await,
        Command::Setchannel(arg) => handle_setchannel(&bot, &state, actor, &arg).await,
        Command::Settags(arg) => handle_settags(&bot, &state, actor, &arg).await,
        Command::Addtags(arg) => handle_edit_tags(&bot, &state, actor, &arg, TagEdit::Add).await,
        Command::Deltags(arg) => handle_edit_tags(&bot, &state, actor, &arg, TagEdit::Remove).await,
        Command::Setinterval(arg) => handle_setinterval(&bot, &state, actor, &arg).await,
        Command::Setrules(arg) => handle_setrules(&bot, &state, actor, &arg).await,
        Command::Addrules(arg) => handle_addrules(&bot, &state, actor, &arg).await,
        Command::Delrules(arg) => handle_delrules(&bot, &state, actor, &arg).await,
        Command::Delposter(arg) => handle_delposter(&state, actor, &arg).await,
        Command::Posters => handle_posters(&state, actor).await,
        Command::Announcements(arg) => handle_announcements(&bot, &state, actor, &arg).await,
        Command::Spotlight(arg) => handle_spotlight(&bot, &state, actor, &arg).await,
        Command::Announcemute(arg) => handle_announce_mute(&bot, &state, actor, &arg, true).await,
        Command::Announceunmute(arg) => {
            handle_announce_mute(&bot, &state, actor, &arg, false).await
        }
    };

    if !reply.is_empty() {
        bot.send_message(msg.chat.id, reply)
            .link_preview_options(no_preview())
            .await?;
    }
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
/// Who is submitting, as Telegram sees them at this moment.
struct Submitter {
    id: TelegramId,
    display_name: Option<String>,
    username: Option<String>,
}

impl From<&TgUser> for Submitter {
    fn from(from: &TgUser) -> Self {
        Self {
            id: TelegramId::from(from.id.0 as i64),
            display_name: Some(from.full_name()),
            username: from.username.clone(),
        }
    }
}

async fn submit(
    bot: &Bot,
    state: &SharedState,
    submitter: Submitter,
    url: Url,
    tags: Vec<Tag>,
    forward: Option<PendingForward>,
) -> String {
    use domain::elements::telegram::{TelegramCopyRef, TelegramCopyRepository as _};
    use teloxide::payloads::CopyMessageSetters as _;
    use teloxide::types::MessageId;

    let submitter_name = submitter
        .display_name
        .clone()
        .unwrap_or_else(|| "a user".to_string());
    // Moderators see who to talk to (or /ban): @username, or the raw id
    // when the account has no public handle. Channel captions stay
    // name-only — this handle is moderation-facing.
    let submitter_contact = match &submitter.username {
        Some(handle) => format!("{submitter_name} (@{handle})"),
        None => format!("{submitter_name} (id {})", submitter.id.as_ref()),
    };
    let outcome = suggest::handle(
        SuggestCommand {
            submitter: submitter.id,
            display_name: submitter.display_name,
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
            state.pending.lock().await.insert(
                *submitter.id.as_ref(),
                PendingSubmission {
                    url,
                    forward,
                    direct_add: false,
                },
            );
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
                "New submission #{}\n{origin_line}\nTags: {tag_line}\nSubmitted by {submitter_contact}",
                post.id
            );
            // Reviewers should see the actual media, not just a link:
            // resolve through the same pipeline the publisher uses.
            let review_media = match &forward {
                Some(_) => None, // forwards are re-copied below, media included
                None => {
                    use domain::elements::media::MediaResolver as _;
                    match state.resolver.resolve(&post.source).await {
                        Ok(media) => Some(media),
                        Err(e) => {
                            tracing::debug!(
                                event = %Event::MediaLinkFallback, post_id = %post.id,
                                error = %e, "review media resolution failed; sending text"
                            );
                            None
                        }
                    }
                }
            };
            for reviewer in &reviewers {
                use domain::elements::media::ResolvedMedia;
                use teloxide::types::InputFile;

                let reviewer_chat = ChatId(*reviewer.telegram_id.as_ref());
                let sent = match (&forward, &review_media) {
                    (Some(fwd), _) => bot
                        .copy_message(
                            reviewer_chat,
                            ChatId(fwd.origin_chat_id),
                            MessageId(fwd.origin_message_id),
                        )
                        .caption(text.clone())
                        .reply_markup(review_keyboard(post.id))
                        .await
                        .map(|_| ()),
                    (None, Some(ResolvedMedia::Photo(media_url))) => bot
                        .send_photo(reviewer_chat, InputFile::url(media_url.clone()))
                        .caption(text.clone())
                        .reply_markup(review_keyboard(post.id))
                        .await
                        .map(|_| ()),
                    (None, Some(ResolvedMedia::Video(media_url))) => bot
                        .send_video(reviewer_chat, InputFile::url(media_url.clone()))
                        .caption(text.clone())
                        .reply_markup(review_keyboard(post.id))
                        .await
                        .map(|_| ()),
                    (None, Some(ResolvedMedia::Animation(media_url))) => bot
                        .send_animation(reviewer_chat, InputFile::url(media_url.clone()))
                        .caption(text.clone())
                        .reply_markup(review_keyboard(post.id))
                        .await
                        .map(|_| ()),
                    // Link media / no resolution: text with the default
                    // link preview doing its best.
                    _ => bot
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

async fn handle_suggest(bot: &Bot, state: &SharedState, submitter: Submitter, arg: &str) -> String {
    let mut parts = arg.split_whitespace();
    let Some(url) = parts.next().and_then(|raw| Url::parse(raw).ok()) else {
        return "Usage: /suggest <source-url> [tags…] — e621, FurAffinity, Twitter/X, BlueSky, \
                DeviantArt and t.me links are accepted. Non-e621 sources need tags \
                (I'll ask if you leave them off)."
            .to_string();
    };
    let tags: Vec<Tag> = parts.map(Tag::from).collect();
    submit(bot, state, submitter, url, tags, None).await
}

/// Completes in-flight dialogues: moderation follow-ups (rejection reason,
/// extra tags) take priority, then pending submissions awaiting tags.
pub async fn handle_pending_tags(bot: Bot, msg: Message, state: SharedState) -> ResponseResult<()> {
    let Some((from, actor)) = sender(&msg) else {
        return Ok(());
    };
    let Some(text) = msg.text() else {
        return Ok(());
    };

    // Moderation dialogues first.
    if let Some(dialogue) = state.pending_moderation.lock().await.remove(actor.as_ref()) {
        let reply = complete_moderation_dialogue(&bot, &state, actor, dialogue, text).await;
        bot.send_message(msg.chat.id, reply)
            .link_preview_options(no_preview())
            .await?;
        return Ok(());
    }

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
    let reply = if pending.direct_add {
        complete_direct_save(&state, actor, pending.url, tags).await
    } else {
        submit(
            &bot,
            &state,
            Submitter::from(from),
            pending.url,
            tags,
            pending.forward,
        )
        .await
    };
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

fn describe_user(user: &Option<domain::elements::user::User>) -> String {
    match user {
        None => "—".to_string(),
        Some(user) => {
            let name = user
                .display_name
                .clone()
                .unwrap_or_else(|| "unnamed".into());
            format!("{name} (id {}, {})", user.telegram_id.as_ref(), user.role)
        }
    }
}

async fn handle_postinfo(state: &SharedState, actor: TelegramId, arg: &str) -> String {
    use application::commands::post_info::post_info;

    let Some(post_id) = parse_post_id(arg) else {
        return "Usage: /postinfo <post-id>".to_string();
    };
    match post_info(
        actor,
        post_id,
        &state.users,
        &state.posts,
        &state.publications,
        &state.reports,
    )
    .await
    {
        Err(e) => describe(e),
        Ok(info) => {
            let post = &info.post;
            let mut lines = vec![
                format!(
                    "Post #{} — {}{}",
                    post.id,
                    post.status,
                    post.feed_position
                        .map(|p| format!(" (feed position {p})"))
                        .unwrap_or_default()
                ),
                format!("Source: {}", post.source.as_ref()),
                format!(
                    "Submitted: {} by {}",
                    post.submitted_at.format("%Y-%m-%d %H:%M UTC"),
                    describe_user(&info.submitter)
                ),
                match post.moderated_at {
                    Some(at) => format!(
                        "Moderated: {} by {}",
                        at.format("%Y-%m-%d %H:%M UTC"),
                        describe_user(&info.moderator)
                    ),
                    None => "Moderated: —".to_string(),
                },
                format!(
                    "Tags ({}): {}",
                    post.tags.len(),
                    post.tags
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
                format!(
                    "Artists: {}",
                    if post.artists.is_empty() {
                        "—".to_string()
                    } else {
                        post.artists
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                ),
                format!(
                    "Last posted: {}",
                    post.last_posted
                        .map(|at| at.format("%Y-%m-%d %H:%M UTC").to_string())
                        .unwrap_or_else(|| "never".to_string())
                ),
                format!("Reports: {}", info.report_count),
            ];
            if info.publications.is_empty() {
                lines.push("Published: never".to_string());
            } else {
                lines.push(format!("Published {} time(s):", info.publications.len()));
                for publication in &info.publications {
                    lines.push(format!(
                        "  • chat {} msg {} at {}",
                        publication.chat_id,
                        publication.message_id,
                        publication.published_at.format("%Y-%m-%d %H:%M UTC")
                    ));
                }
            }
            lines.join("\n")
        }
    }
}

async fn moderate_reply(
    bot: &Bot,
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
        Ok(post) => {
            if approve {
                notify_submitter_approved(bot, state, &post).await;
            }
            format!("Post #{} is now {:?}.", post.id, post.status)
        }
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

/// Moderator direct add: any source, straight into the feed.
async fn handle_save(state: &SharedState, actor: TelegramId, arg: &str) -> String {
    let mut parts = arg.split_whitespace();
    let Some(url) = parts.next().and_then(|raw| Url::parse(raw).ok()) else {
        return "Usage: /save <url> [tags…] — any supported source goes straight into the \
                feed. Non-e621 sources need tags (I'll ask if you leave them off)."
            .to_string();
    };
    let tags: Vec<Tag> = parts.map(Tag::from).collect();
    complete_direct_save(state, actor, url, tags).await
}

async fn complete_direct_save(
    state: &SharedState,
    actor: TelegramId,
    url: Url,
    tags: Vec<Tag>,
) -> String {
    match browse::save(
        SaveCommand {
            actor,
            url: url.clone(),
            tags,
        },
        &state.users,
        &state.posts,
        &*state.e621,
        &state.forbidden,
    )
    .await
    {
        Ok(browse::SaveOutcome::Added(post)) => format!(
            "Added to the feed as #{} (position {}).",
            post.id,
            post.feed_position.unwrap_or_default()
        ),
        Ok(browse::SaveOutcome::TagsNeeded) => {
            state.pending.lock().await.insert(
                *actor.as_ref(),
                PendingSubmission {
                    url,
                    forward: None,
                    direct_add: true,
                },
            );
            "Reply with the tags for this post (space-separated) — it goes straight into \
             the feed."
                .to_string()
        }
        Err(e) => describe(e),
    }
}

/// Good news travels: tell the submitter their post made it into the feed.
/// (Rejections stay silent unless the moderator used Reject-with-reason.)
async fn notify_submitter_approved(
    bot: &Bot,
    state: &SharedState,
    post: &domain::elements::post::Post,
) {
    use domain::elements::user::UserRepository as _;

    let Some(user_id) = post.submitted_by else {
        return;
    };
    let Ok(Some(user)) = state.users.find_by_id(user_id).await else {
        return;
    };
    match bot
        .send_message(
            ChatId(*user.telegram_id.as_ref()),
            format!(
                "🎉 Your submission #{} was approved — it will be posted when a matching \
                 channel's turn comes up!",
                post.id
            ),
        )
        .await
    {
        Ok(_) => tracing::info!(
            event = %Event::SubmitterNotified, post_id = %post.id, "approval relayed to submitter"
        ),
        Err(e) => tracing::debug!(post_id = %post.id, error = %e, "approval DM failed"),
    }
}

/// Send one page of browse previews; returns how many were delivered.
async fn send_browse_page(
    bot: &Bot,
    chat: ChatId,
    state: &SharedState,
    actor: TelegramId,
    tags: Vec<Tag>,
    page: u32,
    count: usize,
) -> Result<usize, String> {
    use teloxide::types::InputFile;

    let results = match browse::search(
        BrowseCommand { actor, tags, page },
        &state.users,
        &*state.e621,
        &state.forbidden,
        &state.required,
    )
    .await
    {
        Ok(results) => results,
        Err(e) => return Err(describe(e)),
    };
    if results.is_empty() {
        return Ok(0);
    }
    // Like the legacy bot: each result is its own photo with the
    // Send / sources / Erase keyboard.
    let mut sent = 0usize;
    for metadata in results.iter().take(count) {
        use domain::elements::media::ResolvedMedia;

        let e621_url = metadata.source.as_ref();
        let Some(e621_id) = e621_url
            .path_segments()
            .and_then(|mut s| s.nth(1))
            .and_then(|id| id.parse::<u64>().ok())
        else {
            continue;
        };
        let keyboard = browse_keyboard(e621_id, e621_url, &metadata.artist_sources);

        // Preview with the real media type: gifs animate, videos play
        // (via e621's MP4 rendition — Telegram can't fetch webm).
        let animated: Option<Result<(), teloxide::RequestError>> =
            match ResolvedMedia::classify(metadata.file_url.clone()) {
                ResolvedMedia::Animation(gif_url) => Some(
                    bot.send_animation(chat, InputFile::url(gif_url))
                        .reply_markup(keyboard.clone())
                        .await
                        .map(|_| ()),
                ),
                ResolvedMedia::Video(_) => match metadata.mp4_url.clone() {
                    Some(mp4) => Some(
                        bot.send_video(chat, InputFile::url(mp4))
                            .reply_markup(keyboard.clone())
                            .await
                            .map(|_| ()),
                    ),
                    None => None,
                },
                _ => None,
            };

        let outcome = match animated {
            Some(Ok(())) => Ok(()),
            Some(Err(e)) => {
                tracing::debug!(
                    event = %Event::MediaLinkFallback, source = %e621_url, error = %e,
                    "animated preview refused; falling back to still"
                );
                bot.send_photo(chat, InputFile::url(metadata.preview_url.clone()))
                    .reply_markup(keyboard)
                    .await
                    .map(|_| ())
            }
            None => bot
                .send_photo(chat, InputFile::url(metadata.preview_url.clone()))
                .reply_markup(keyboard)
                .await
                .map(|_| ()),
        };
        match outcome {
            Ok(()) => sent += 1,
            Err(e) => tracing::warn!(
                event = %Event::BrowseAlbumFailed, source = %e621_url, error = %e,
                "browse preview send failed"
            ),
        }
    }
    Ok(sent)
}

/// The "More ➡" control under the page summary.
fn browse_more_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([[InlineKeyboardButton::callback("More ➡", "brmore")]])
}

async fn handle_browse(
    bot: &Bot,
    chat: ChatId,
    state: &SharedState,
    actor: TelegramId,
    arg: &str,
) -> String {
    // Optional leading count: /browse 12 wolf male  (1..=20, default 5).
    let mut parts = arg.split_whitespace().peekable();
    let count = match parts.peek().and_then(|raw| raw.parse::<usize>().ok()) {
        Some(n) if (1..=20).contains(&n) => {
            parts.next();
            n
        }
        _ => 5,
    };
    let tags: Vec<Tag> = parts.map(Tag::from).collect();

    match send_browse_page(bot, chat, state, actor, tags.clone(), 1, count).await {
        Err(reply) => reply,
        Ok(0) => "No matching e621 posts.".to_string(),
        Ok(sent) => {
            state.browse_sessions.lock().await.insert(
                *actor.as_ref(),
                BrowseSession {
                    tags,
                    next_page: 2,
                    count,
                },
            );
            let summary = format!("{sent} results — Send saves to the pool, Erase dismisses.");
            if let Err(e) = bot
                .send_message(chat, summary)
                .reply_markup(browse_more_keyboard())
                .await
            {
                tracing::warn!(event = %Event::BrowseAlbumFailed, error = %e, "summary send failed");
            }
            String::new()
        }
    }
}

enum TagPolicyAction {
    Forbid,
    Unforbid,
    Require,
    Unrequire,
    Spoiler,
    Unspoiler,
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
            TagPolicyAction::Spoiler => "spoiler",
            TagPolicyAction::Unspoiler => "unspoiler",
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
        TagPolicyAction::Spoiler => {
            use domain::elements::tag_policy::SpoilerTagRepository as _;
            state
                .spoilers
                .add(tag.clone())
                .await
                .map_err(|e| e.to_string())
        }
        TagPolicyAction::Unspoiler => {
            use domain::elements::tag_policy::SpoilerTagRepository as _;
            state.spoilers.remove(&tag).await.map_err(|e| e.to_string())
        }
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
/// Parse a subscription tag list: bare tags (all required), `(a b)` OR-groups
/// (at least one hit per group), and top-level `-tag` (forbidden). A `-tag`
/// inside a group stays part of that group's disjunction.
fn parse_tag_lists<'a>(
    parts: impl Iterator<Item = &'a str>,
) -> Result<(Vec<domain::elements::tag_rule::TagTerm>, Vec<Tag>), String> {
    use domain::elements::tag_rule::{TagLiteral, TagTerm};

    let raw = parts.collect::<Vec<_>>().join(" ");
    if raw.contains('[') || raw.contains(']') {
        return Err(
            "That looks like a conditional rule — [if]->[then] rules go in /setrules, not here."
                .to_string(),
        );
    }
    let terms = TagTerm::parse_list(&raw).map_err(|e| format!("Bad tag syntax: {e}"))?;
    let mut subscribed = Vec::new();
    let mut forbidden = Vec::new();
    for term in terms {
        match term.0.as_slice() {
            [TagLiteral::Lacks(tag)] => forbidden.push(tag.clone()),
            _ => subscribed.push(term),
        }
    }
    Ok((subscribed, forbidden))
}

/// Resolve a poster reference: `#7`/`7` = poster id; `@channel` or a
/// (negative) chat id = every poster bound to that chat.
async fn resolve_posters(
    bot: &Bot,
    state: &SharedState,
    token: &str,
) -> Result<Vec<domain::elements::poster::PosterId>, String> {
    use domain::elements::poster::PosterId;
    use domain::elements::publisher_config::PublisherConfigRepository as _;

    if let Ok(id) = token.trim_start_matches('#').parse::<u64>() {
        return Ok(vec![PosterId::from(id)]);
    }
    let chat_id = if let Ok(id) = token.parse::<i64>() {
        id
    } else {
        let resolver = BotUserResolver { bot: bot.clone() };
        match resolve_target(&resolver, token).await {
            Ok(Some(id)) => *id.as_ref(),
            Ok(None) => return Err(format!("Can't resolve {token}.")),
            Err(e) => return Err(e),
        }
    };
    let posters: Vec<PosterId> = state
        .publisher_configs
        .list_all()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|config| config.chat_id == chat_id)
        .map(|config| config.poster_id)
        .collect();
    if posters.is_empty() {
        return Err(format!("No poster is bound to {token} — see /posters."));
    }
    Ok(posters)
}

/// One readable block per poster — every management command answers with
/// the poster's full picture instead of a one-line prose summary.
async fn poster_summary(
    state: &SharedState,
    poster: &domain::elements::poster::Poster,
    headline: &str,
) -> String {
    use domain::elements::publisher_config::PublisherConfigRepository as _;

    let join = |items: Vec<String>| items.join(", ");
    let mut lines = if headline.is_empty() {
        vec![format!("Poster #{}", poster.id)]
    } else {
        vec![format!("Poster #{} — {headline}", poster.id)]
    };
    lines.push(format!(
        "⏱ Interval: every {} min",
        poster.time_interval.as_ref()
    ));
    match state.publisher_configs.find_by_poster(poster.id).await {
        Ok(Some(config)) if config.receive_announcements => {
            lines.push(format!("📍 Posts to: chat {}", config.chat_id));
        }
        Ok(Some(config)) => lines.push(format!(
            "📍 Posts to: chat {} (announcements muted)",
            config.chat_id
        )),
        _ => lines.push("📍 Posts to: nowhere — bind with /setchannel".to_string()),
    }
    if poster.subscribed_tags.is_empty() {
        lines.push("🏷 Tags: anything".to_string());
    } else {
        lines.push(format!(
            "🏷 Tags: {}",
            join(
                poster
                    .subscribed_tags
                    .iter()
                    .map(ToString::to_string)
                    .collect()
            )
        ));
    }
    if !poster.forbidden_tags.is_empty() {
        lines.push(format!(
            "🚫 Never: {}",
            join(
                poster
                    .forbidden_tags
                    .iter()
                    .map(ToString::to_string)
                    .collect()
            )
        ));
    }
    if !poster.rules.is_empty() {
        lines.push("📐 Rules:".to_string());
        for (index, rule) in poster.rules.iter().enumerate() {
            let side = |terms: &[domain::elements::tag_rule::TagTerm]| {
                terms
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            lines.push(format!(
                "  {}. [{}] → [{}]",
                index + 1,
                side(&rule.if_all),
                side(&rule.then_all)
            ));
        }
    }
    lines.join("\n")
}

async fn handle_setinterval(
    bot: &Bot,
    state: &SharedState,
    actor: TelegramId,
    arg: &str,
) -> String {
    let mut parts = arg.split_whitespace();
    let (Some(target), Some(raw_minutes), None) = (parts.next(), parts.next(), parts.next()) else {
        return "Usage: /setinterval <poster|@channel|chat-id> <minutes>\n\
                Minutes must divide 60 (1,2,3,4,5,6,10,12,15,20,30,60)."
            .to_string();
    };
    let interval = match raw_minutes.parse::<u8>().map(PostInterval::new) {
        Ok(Ok(interval)) => interval,
        Ok(Err(e)) => return e.to_string(),
        Err(_) => return "Minutes must be a number.".to_string(),
    };
    let poster_ids = match resolve_posters(bot, state, target).await {
        Ok(ids) => ids,
        Err(e) => return e,
    };
    let mut lines = Vec::new();
    for poster_id in poster_ids {
        match manage_poster::set_interval(actor, poster_id, interval, &state.users, &state.posters)
            .await
        {
            Ok(poster) => lines.push(
                poster_summary(state, &poster, "interval updated, live within a minute").await,
            ),
            Err(e) => lines.push(describe(e)),
        }
    }
    lines.join("\n\n")
}

async fn handle_settags(bot: &Bot, state: &SharedState, actor: TelegramId, arg: &str) -> String {
    let mut parts = arg.split_whitespace();
    let Some(target) = parts.next() else {
        return "Usage: /settags <poster|@channel|chat-id> [tags… (or groups…) -forbidden…]\n\
                `(gay bisexual)` = at least one of the group must be present.\n\
                No tags = post anything (subscription filter removed).\n\
                To change a few without rewriting: /addtags and /deltags."
            .to_string();
    };
    let poster_ids = match resolve_posters(bot, state, target).await {
        Ok(ids) => ids,
        Err(e) => return e,
    };
    let (subscribed, forbidden) = match parse_tag_lists(parts) {
        Ok(lists) => lists,
        Err(e) => return e,
    };
    let mut lines = Vec::new();
    for poster_id in poster_ids {
        match manage_poster::set_tags(
            manage_poster::SetTags {
                actor,
                poster_id,
                subscribed_tags: subscribed.clone(),
                forbidden_tags: forbidden.clone(),
            },
            &state.users,
            &state.posters,
        )
        .await
        {
            Ok(poster) => lines
                .push(poster_summary(state, &poster, "tags updated, live within a minute").await),
            Err(e) => lines.push(describe(e)),
        }
    }
    lines.join("\n\n")
}

#[derive(Clone, Copy, PartialEq)]
enum TagEdit {
    Add,
    Remove,
}

/// `/addtags` and `/deltags`: merge into (or strip from) the stored lists
/// instead of replacing them. The same argument string given to /addtags is
/// undone by giving it to /deltags.
async fn handle_edit_tags(
    bot: &Bot,
    state: &SharedState,
    actor: TelegramId,
    arg: &str,
    edit: TagEdit,
) -> String {
    use domain::elements::poster::PosterRepository as _;

    let mut parts = arg.split_whitespace();
    let Some(target) = parts.next() else {
        let verb = match edit {
            TagEdit::Add => "/addtags",
            TagEdit::Remove => "/deltags",
        };
        return format!(
            "Usage: {verb} <poster|@channel|chat-id> [tags… (or groups…) -forbidden…]\n\
             Only the listed entries change; everything else stays."
        );
    };
    let poster_ids = match resolve_posters(bot, state, target).await {
        Ok(ids) => ids,
        Err(e) => return e,
    };
    let (terms, forbidden) = match parse_tag_lists(parts) {
        Ok(lists) => lists,
        Err(e) => return e,
    };
    if terms.is_empty() && forbidden.is_empty() {
        return "Nothing to change — list at least one tag.".to_string();
    }
    let mut lines = Vec::new();
    for poster_id in poster_ids {
        let poster = match state.posters.find_by_id(poster_id).await {
            Ok(Some(p)) => p,
            Ok(None) => {
                lines.push(format!("Poster #{poster_id} does not exist."));
                continue;
            }
            Err(e) => {
                lines.push(e.to_string());
                continue;
            }
        };
        let mut subscribed = poster.subscribed_tags.clone();
        let mut forbidden_now = poster.forbidden_tags.clone();
        let mut untouched = Vec::new();
        match edit {
            TagEdit::Add => {
                for term in &terms {
                    if subscribed.contains(term) {
                        untouched.push(term.to_string());
                    } else {
                        subscribed.push(term.clone());
                    }
                }
                for tag in &forbidden {
                    if forbidden_now.contains(tag) {
                        untouched.push(format!("-{tag}"));
                    } else {
                        forbidden_now.push(tag.clone());
                    }
                }
            }
            TagEdit::Remove => {
                for term in &terms {
                    if subscribed.contains(term) {
                        subscribed.retain(|t| t != term);
                    } else {
                        untouched.push(term.to_string());
                    }
                }
                for tag in &forbidden {
                    if forbidden_now.contains(tag) {
                        forbidden_now.retain(|t| t != tag);
                    } else {
                        untouched.push(format!("-{tag}"));
                    }
                }
            }
        }
        match manage_poster::set_tags(
            manage_poster::SetTags {
                actor,
                poster_id,
                subscribed_tags: subscribed,
                forbidden_tags: forbidden_now,
            },
            &state.users,
            &state.posters,
        )
        .await
        {
            Ok(poster) => {
                let mut block =
                    poster_summary(state, &poster, "tags updated, live within a minute").await;
                if !untouched.is_empty() {
                    let what = match edit {
                        TagEdit::Add => "already there",
                        TagEdit::Remove => "not found",
                    };
                    block.push_str(&format!("\n⚠️ {what}: {}", untouched.join(", ")));
                }
                lines.push(block);
            }
            Err(e) => lines.push(describe(e)),
        }
    }
    lines.join("\n\n")
}

async fn handle_setrules(bot: &Bot, state: &SharedState, actor: TelegramId, arg: &str) -> String {
    use domain::elements::tag_rule::TagRule;

    let arg = arg.trim();
    let Some(target) = arg.split_whitespace().next() else {
        return "Usage: /setrules <poster|@channel|chat-id> [if-tags…]->[then-tags…] …\n\
                Example: /setrules @straightchannel [solo]->[-male]\n\
                `-tag` means the tag must be absent. No rules = clear all rules.\n\
                To change a few without rewriting: /addrules and /delrules <n>."
            .to_string();
    };
    let poster_ids = match resolve_posters(bot, state, target).await {
        Ok(ids) => ids,
        Err(e) => return e,
    };
    let rules = match TagRule::parse_all(&arg[target.len()..]) {
        Ok(rules) => rules,
        Err(e) => return format!("Bad rule syntax: {e}"),
    };
    let mut lines = Vec::new();
    for poster_id in poster_ids {
        match manage_poster::set_rules(
            actor,
            poster_id,
            rules.clone(),
            &state.users,
            &state.posters,
        )
        .await
        {
            Ok(poster) if poster.rules.is_empty() => lines
                .push(poster_summary(state, &poster, "rules cleared, live within a minute").await),
            Ok(poster) => lines
                .push(poster_summary(state, &poster, "rules updated, live within a minute").await),
            Err(e) => lines.push(describe(e)),
        }
    }
    lines.join("\n\n")
}

async fn handle_addrules(bot: &Bot, state: &SharedState, actor: TelegramId, arg: &str) -> String {
    use domain::elements::poster::PosterRepository as _;
    use domain::elements::tag_rule::TagRule;

    let arg = arg.trim();
    let Some(target) = arg.split_whitespace().next() else {
        return "Usage: /addrules <poster|@channel|chat-id> [if-tags…]->[then-tags…] …\n\
                Appends to the existing rules; /delrules removes by number."
            .to_string();
    };
    let poster_ids = match resolve_posters(bot, state, target).await {
        Ok(ids) => ids,
        Err(e) => return e,
    };
    let added = match TagRule::parse_all(&arg[target.len()..]) {
        Ok(rules) if rules.is_empty() => return "List at least one [if]->[then] rule.".to_string(),
        Ok(rules) => rules,
        Err(e) => return format!("Bad rule syntax: {e}"),
    };
    let mut lines = Vec::new();
    for poster_id in poster_ids {
        let poster = match state.posters.find_by_id(poster_id).await {
            Ok(Some(p)) => p,
            Ok(None) => {
                lines.push(format!("Poster #{poster_id} does not exist."));
                continue;
            }
            Err(e) => {
                lines.push(e.to_string());
                continue;
            }
        };
        let mut rules = poster.rules.clone();
        let mut already = Vec::new();
        for rule in &added {
            if rules.contains(rule) {
                already.push(rule.to_string());
            } else {
                rules.push(rule.clone());
            }
        }
        match manage_poster::set_rules(actor, poster_id, rules, &state.users, &state.posters).await
        {
            Ok(poster) => {
                let mut block =
                    poster_summary(state, &poster, "rules updated, live within a minute").await;
                if !already.is_empty() {
                    block.push_str(&format!("\n⚠️ already there: {}", already.join(" ")));
                }
                lines.push(block);
            }
            Err(e) => lines.push(describe(e)),
        }
    }
    lines.join("\n\n")
}

async fn handle_delrules(bot: &Bot, state: &SharedState, actor: TelegramId, arg: &str) -> String {
    use domain::elements::poster::PosterRepository as _;

    let mut parts = arg.split_whitespace();
    let Some(target) = parts.next() else {
        return "Usage: /delrules <poster|@channel|chat-id> <n…>\n\
                Numbers as shown by /posters (1 = first rule)."
            .to_string();
    };
    let poster_ids = match resolve_posters(bot, state, target).await {
        Ok(ids) => ids,
        Err(e) => return e,
    };
    let mut indices: Vec<usize> = Vec::new();
    for raw in parts {
        match raw.parse::<usize>() {
            Ok(n) if n >= 1 => indices.push(n),
            _ => return format!("'{raw}' is not a rule number (1 = first rule)."),
        }
    }
    if indices.is_empty() {
        return "Which rule? Give its number as shown by /posters (1 = first rule).".to_string();
    }
    indices.sort_unstable();
    indices.dedup();
    let mut lines = Vec::new();
    for poster_id in poster_ids {
        let poster = match state.posters.find_by_id(poster_id).await {
            Ok(Some(p)) => p,
            Ok(None) => {
                lines.push(format!("Poster #{poster_id} does not exist."));
                continue;
            }
            Err(e) => {
                lines.push(e.to_string());
                continue;
            }
        };
        let out_of_range: Vec<String> = indices
            .iter()
            .filter(|n| **n > poster.rules.len())
            .map(ToString::to_string)
            .collect();
        let rules: Vec<_> = poster
            .rules
            .iter()
            .enumerate()
            .filter(|(i, _)| !indices.contains(&(i + 1)))
            .map(|(_, rule)| rule.clone())
            .collect();
        match manage_poster::set_rules(actor, poster_id, rules, &state.users, &state.posters).await
        {
            Ok(poster) => {
                let headline = if poster.rules.is_empty() {
                    "rules cleared, live within a minute"
                } else {
                    "rules updated, live within a minute"
                };
                let mut block = poster_summary(state, &poster, headline).await;
                if !out_of_range.is_empty() {
                    block.push_str(&format!("\n⚠️ no rule number {}", out_of_range.join(", ")));
                }
                lines.push(block);
            }
            Err(e) => lines.push(describe(e)),
        }
    }
    lines.join("\n\n")
}

async fn handle_newposter(bot: &Bot, state: &SharedState, actor: TelegramId, arg: &str) -> String {
    const USAGE: &str = "Usage: /newposter <interval-minutes> <@channel|chat-id> [tags… -forbidden…]\n\
        Interval must divide 60 (1,2,3,4,5,6,10,12,15,20,30,60). \
        No tags = post anything; `(gay bisexual)` groups need one hit. \
        The bot must be an admin of the channel.";

    let mut parts = arg.split_whitespace();
    let Some(interval) = parts.next().and_then(|v| v.parse::<u8>().ok()) else {
        return USAGE.to_string();
    };
    let interval = match PostInterval::new(interval) {
        Ok(i) => i,
        Err(e) => return e.to_string(),
    };
    let Some(chat_raw) = parts.next() else {
        return USAGE.to_string();
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
    let (subscribed, forbidden) = match parse_tag_lists(parts) {
        Ok(lists) => lists,
        Err(e) => return e,
    };
    match manage_poster::new_poster(
        NewPoster {
            actor,
            subscribed_tags: subscribed,
            forbidden_tags: forbidden,
            interval,
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
        Ok(poster) => poster_summary(state, &poster, "created, live within a minute").await,
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
            format!("Poster #{poster_id} now publishes to {chat_id} — live within a minute.")
        }
        Err(e) => describe(e),
    }
}

async fn handle_delposter(state: &SharedState, actor: TelegramId, arg: &str) -> String {
    let Some(poster_id) = arg
        .trim()
        .trim_start_matches('#')
        .parse::<u64>()
        .ok()
        .map(domain::elements::poster::PosterId::from)
    else {
        return "Usage: /delposter <poster-id> — see /posters for the ids.".to_string();
    };
    match manage_poster::delete_poster(
        actor,
        poster_id,
        &state.users,
        &state.posters,
        &state.publisher_configs,
    )
    .await
    {
        Ok(()) => format!(
            "Poster #{poster_id} deleted — it stops firing within a minute. \
             The feed and its posts are untouched."
        ),
        Err(e) => describe(e),
    }
}

async fn handle_announcements(
    bot: &Bot,
    state: &SharedState,
    actor: TelegramId,
    arg: &str,
) -> String {
    use application::commands::auth::require_role;
    use domain::elements::announcement::AnnouncementRepository as _;

    if let Err(e) = require_role(&state.users, actor, Role::Owner).await {
        return describe(e);
    }
    match arg.trim().to_lowercase().as_str() {
        "now" => match crate::announcer::announce_round(state, bot).await {
            Ok((sent, 0)) => format!("Announced to {sent} channel(s)."),
            Ok((sent, failed)) => {
                format!("Announced to {sent} channel(s); {failed} delivery(ies) failed — see logs.")
            }
            Err(reason) => format!("Nothing announced: {reason}."),
        },
        "off" | "0" => match state.announcements.set_interval_hours(0).await {
            Ok(()) => {
                tracing::info!(event = %Event::AnnouncementConfigChanged, interval_hours = 0u32, "announcements disabled");
                "Recurring announcements disabled.".to_string()
            }
            Err(e) => e.to_string(),
        },
        raw => match raw.parse::<u32>() {
            Ok(hours) if hours > 0 => match state.announcements.set_interval_hours(hours).await {
                Ok(()) => {
                    tracing::info!(event = %Event::AnnouncementConfigChanged, interval_hours = hours, "announcement cadence set");
                    format!(
                        "Announcements every {hours}h. Next round fires within a minute of \
                         becoming due (first one immediately if none was ever sent)."
                    )
                }
                Err(e) => e.to_string(),
            },
            _ => "Usage: /announcements <hours|now|off>".to_string(),
        },
    }
}

async fn handle_spotlight(bot: &Bot, state: &SharedState, actor: TelegramId, arg: &str) -> String {
    use application::commands::auth::require_role;
    use domain::elements::announcement::AnnouncementRepository as _;
    use domain::elements::publisher_config::PublisherConfigRepository as _;

    if let Err(e) = require_role(&state.users, actor, Role::Owner).await {
        return describe(e);
    }
    let raw = arg.trim();
    if raw.is_empty() {
        return "Usage: /spotlight <@channel|chat-id|off>".to_string();
    }
    if raw.eq_ignore_ascii_case("off") {
        return match state.announcements.set_spotlight(None).await {
            Ok(()) => {
                tracing::info!(event = %Event::AnnouncementConfigChanged, "spotlight cleared");
                "Spotlight cleared.".to_string()
            }
            Err(e) => e.to_string(),
        };
    }
    let chat_id = if let Ok(id) = raw.parse::<i64>() {
        id
    } else {
        let resolver = BotUserResolver { bot: bot.clone() };
        match resolve_target(&resolver, raw).await {
            Ok(Some(id)) => *id.as_ref(),
            Ok(None) => return format!("Can't resolve {raw}."),
            Err(e) => return e,
        }
    };
    let bound = state
        .publisher_configs
        .list_all()
        .await
        .map(|configs| configs.iter().any(|c| c.chat_id == chat_id))
        .unwrap_or(false);
    match state.announcements.set_spotlight(Some(chat_id)).await {
        Ok(()) => {
            tracing::info!(event = %Event::AnnouncementConfigChanged, spotlight = chat_id, "spotlight set");
            if bound {
                format!("Spotlight set: chat {chat_id} tops the next directory.")
            } else {
                format!(
                    "Spotlight set to chat {chat_id} — note it is not currently a consuming \
                     channel, so it won't appear in the directory until a poster is bound to it."
                )
            }
        }
        Err(e) => e.to_string(),
    }
}

async fn handle_announce_mute(
    bot: &Bot,
    state: &SharedState,
    actor: TelegramId,
    arg: &str,
    muted: bool,
) -> String {
    let raw = arg.trim();
    if raw.is_empty() {
        return "Usage: /announcemute <@channel|chat-id> (or /announceunmute)".to_string();
    }
    let chat_id = if let Ok(id) = raw.parse::<i64>() {
        id
    } else {
        let resolver = BotUserResolver { bot: bot.clone() };
        match resolve_target(&resolver, raw).await {
            Ok(Some(id)) => *id.as_ref(),
            Ok(None) => return format!("Can't resolve {raw}."),
            Err(e) => return e,
        }
    };
    match manage_poster::set_announcement_mute(
        actor,
        chat_id,
        muted,
        &state.users,
        &state.publisher_configs,
    )
    .await
    {
        Ok(_) if muted => format!(
            "Chat {chat_id} will no longer receive announcements — it still appears in the \
             directory sent to other channels."
        ),
        Ok(_) => format!("Chat {chat_id} receives announcements again."),
        Err(e) => describe(e),
    }
}

async fn handle_posters(state: &SharedState, actor: TelegramId) -> String {
    use application::commands::auth::require_role;
    use domain::elements::poster::PosterRepository;

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
        lines.push(poster_summary(state, &poster, "").await);
    }
    lines.join("\n\n")
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
        Submitter::from(from),
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
        // Dialogue buttons: the moderator's next message completes the
        // action (rejection reason / extra tags).
        ["mod", verb @ ("reason" | "addtags"), id] => {
            use crate::state::ModerationDialogue;
            use teloxide::payloads::AnswerCallbackQuerySetters as _;

            let toast = match parse_post_id(id) {
                None => "Malformed callback.".to_string(),
                Some(post_id) => {
                    match application::commands::auth::require_role(
                        &state.users,
                        actor,
                        Role::Moderator,
                    )
                    .await
                    {
                        Err(e) => describe(e),
                        Ok(_) => {
                            let (dialogue, event, prompt) = if verb == "reason" {
                                (
                                    ModerationDialogue::RejectReason(post_id),
                                    Event::RejectionReasonRequested,
                                    format!(
                                        "Reply with the reason for rejecting post #{post_id} — \
                                         it will be sent to the submitter."
                                    ),
                                )
                            } else {
                                (
                                    ModerationDialogue::ExtraTags(post_id),
                                    Event::ExtraTagsRequested,
                                    format!(
                                        "Reply with the extra tags for post #{post_id} \
                                         (space-separated) — duplicates are ignored and the \
                                         post is accepted with the merged set."
                                    ),
                                )
                            };
                            state
                                .pending_moderation
                                .lock()
                                .await
                                .insert(*actor.as_ref(), dialogue);
                            tracing::info!(
                                event = %event, post_id = %post_id,
                                telegram_id = actor.as_ref(), "moderation dialogue opened"
                            );
                            if let Some(message) = query.message.as_ref() {
                                let _ = bot.send_message(message.chat().id, prompt).await;
                            }
                            format!("Waiting for your reply for post #{post_id}.")
                        }
                    }
                }
            };
            bot.answer_callback_query(query.id.clone())
                .text(toast)
                .await?;
        }
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
                        Ok(post) => {
                            if verb == "approve" {
                                notify_submitter_approved(&bot, &state, &post).await;
                            }
                            format!("Post #{} → {:?}", post.id, post.status)
                        }
                        Err(e) => describe(e),
                    }
                }
                None => "Malformed callback.".to_string(),
            };
            bot.answer_callback_query(query.id.clone()).await?;
            // Reflect the decision on the DM itself so the buttons disappear.
            if let Some(message) = query.message.as_ref() {
                reflect_outcome_on_dm(&bot, message, &outcome).await;
            }
        }
        // Viewer report button on published posts (legacy messages; new
        // publications use the caption deep link instead).
        ["report", id] => {
            use teloxide::payloads::AnswerCallbackQuerySetters as _;

            let toast = match parse_post_id(id) {
                None => "Malformed report.".to_string(),
                Some(post_id) => file_report(&bot, &state, actor, post_id).await,
            };
            bot.answer_callback_query(query.id.clone())
                .text(toast)
                .await?;
        }
        // Moderator resolution buttons on report DMs.
        ["repmod", verb @ ("take" | "dismiss"), id] => {
            use application::commands::report;
            use teloxide::types::MessageId;

            let outcome = match parse_post_id(id) {
                None => "Malformed callback.".to_string(),
                Some(post_id) if verb == "take" => {
                    match report::take_down(
                        actor,
                        post_id,
                        &state.users,
                        &state.posts,
                        &state.publications,
                    )
                    .await
                    {
                        Ok(deliveries) => {
                            let mut deleted = 0usize;
                            for delivery in &deliveries {
                                match bot
                                    .delete_message(
                                        ChatId(delivery.chat_id),
                                        MessageId(delivery.message_id),
                                    )
                                    .await
                                {
                                    Ok(_) => deleted += 1,
                                    Err(e) => tracing::warn!(
                                        event = %Event::PublishFailed, post_id = %post_id,
                                        chat_id = delivery.chat_id, error = %e,
                                        "channel message delete failed"
                                    ),
                                }
                            }
                            format!(
                                "Post #{post_id} taken down ({deleted}/{} channel message(s) deleted).",
                                deliveries.len()
                            )
                        }
                        Err(e) => describe(e),
                    }
                }
                Some(post_id) => {
                    match report::dismiss(actor, post_id, &state.users, &state.reports).await {
                        Ok(()) => format!("Reports for post #{post_id} dismissed."),
                        Err(e) => describe(e),
                    }
                }
            };
            bot.answer_callback_query(query.id.clone()).await?;
            if let Some(message) = query.message.as_ref() {
                reflect_outcome_on_dm(&bot, message, &outcome).await;
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
                        SaveCommand {
                            actor,
                            url,
                            tags: vec![],
                        },
                        &state.users,
                        &state.posts,
                        &*state.e621,
                        &state.forbidden,
                    )
                    .await
                    {
                        Ok(browse::SaveOutcome::Added(post)) => {
                            format!("Saved to the feed as #{}.", post.id)
                        }
                        Ok(browse::SaveOutcome::TagsNeeded) => {
                            "This source needs tags — use /save <url> <tags…>.".to_string()
                        }
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
        // Browse paging: next page of the moderator's last query.
        ["brmore"] => {
            use teloxide::payloads::AnswerCallbackQuerySetters as _;

            let session = state
                .browse_sessions
                .lock()
                .await
                .get(actor.as_ref())
                .cloned();
            let toast = match session {
                None => "No browse in progress — run /browse first.".to_string(),
                Some(session) => {
                    let chat = query
                        .message
                        .as_ref()
                        .map(|m| m.chat().id)
                        .unwrap_or(ChatId(*actor.as_ref()));
                    match send_browse_page(
                        &bot,
                        chat,
                        &state,
                        actor,
                        session.tags.clone(),
                        session.next_page,
                        session.count,
                    )
                    .await
                    {
                        Err(reply) => reply,
                        Ok(0) => {
                            state.browse_sessions.lock().await.remove(actor.as_ref());
                            "No more results.".to_string()
                        }
                        Ok(sent) => {
                            state.browse_sessions.lock().await.insert(
                                *actor.as_ref(),
                                BrowseSession {
                                    next_page: session.next_page + 1,
                                    ..session
                                },
                            );
                            // A fresh "More ➡" lands BELOW the new page —
                            // the one that was clicked is buried above it.
                            let summary = format!("{sent} more (page {}).", session.next_page);
                            if let Err(e) = bot
                                .send_message(chat, summary)
                                .reply_markup(browse_more_keyboard())
                                .await
                            {
                                tracing::warn!(
                                    event = %Event::BrowseAlbumFailed, error = %e,
                                    "browse page summary send failed"
                                );
                            }
                            String::new()
                        }
                    }
                }
            };
            // The clicked button is stale either way — remove it.
            if let Some(message) = query.message.as_ref() {
                let _ = bot
                    .edit_message_reply_markup(message.chat().id, message.id())
                    .await;
            }
            let mut answer = bot.answer_callback_query(query.id.clone());
            if !toast.is_empty() {
                answer = answer.text(toast);
            }
            answer.await?;
        }
        _ => {
            bot.answer_callback_query(query.id.clone()).await?;
        }
    }
    Ok(())
}
