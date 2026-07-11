//! The Telegram Mini App backend: a JSON API over the same use cases the
//! chat commands run, plus the static SvelteKit bundle.
//!
//! Auth: `Authorization: tma <initData>` (Telegram WebApp signed payload,
//! HMAC-validated against the bot token) or `Authorization: Bearer <token>`
//! (personal tokens from /apitoken — the desktop userscript path). Roles
//! come from the users table either way.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use teloxide::Bot;
use url::Url;

use application::commands::{
    browse, feed, manage_poster, moderate, moderate::ModerateCommand, post_info::post_info, report,
};
use application::selectors::feed::refusal_for;
use domain::elements::{
    e621::E621Fetcher as _,
    media::{MediaResolver as _, ResolvedMedia},
    post::{PostId, PostRepository as _, PostStatus, Source},
    poster::{PosterId, PosterRepository as _},
    publisher_config::PublisherConfigRepository as _,
    tag::Tag,
    tag_policy::{
        ForbiddenTagRepository as _, RequiredTagRepository as _, SpoilerTagRepository as _,
    },
    tag_rule::TagRule,
    user::{Role, TelegramId, User, UserRepository as _},
};
use telemetry::Event;

use crate::commands::{
    Submitter, notify_submitter_approved, poster_summary, poster_verdicts, reject_with_reason,
    request_changes_with_message, resolve_publish_code, submit,
};
use crate::state::SharedState;

#[derive(Clone)]
pub struct WebState {
    pub app: SharedState,
    pub bot: Bot,
    /// The bot token, for initData HMAC validation.
    pub bot_token: String,
}

type ApiError = (StatusCode, Json<Value>);
type ApiResult = Result<Json<Value>, ApiError>;

fn err(status: StatusCode, message: impl std::fmt::Display) -> ApiError {
    (status, Json(json!({ "error": message.to_string() })))
}

fn bad_request(message: impl std::fmt::Display) -> ApiError {
    err(StatusCode::BAD_REQUEST, message)
}

// ---------------------------------------------------------------- auth ----

pub struct Authed {
    pub user: User,
}

/// Validate `Authorization: tma <initData>` per Telegram's spec:
/// secret = HMAC_SHA256(key="WebAppData", bot_token);
/// hash   = hex(HMAC_SHA256(secret, sorted k=v lines minus `hash`)).
fn verify_init_data(init_data: &str, bot_token: &str) -> Result<i64, String> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut provided_hash = String::new();
    for (key, value) in url::form_urlencoded::parse(init_data.as_bytes()) {
        if key == "hash" {
            provided_hash = value.to_string();
        } else {
            pairs.push((key.to_string(), value.to_string()));
        }
    }
    if provided_hash.is_empty() {
        return Err("initData has no hash".to_string());
    }
    pairs.sort();
    let check_string = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut secret = Hmac::<Sha256>::new_from_slice(b"WebAppData").expect("hmac accepts any key");
    secret.update(bot_token.as_bytes());
    let secret = secret.finalize().into_bytes();
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret).expect("hmac accepts any key");
    mac.update(check_string.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    if expected != provided_hash.to_lowercase() {
        return Err("initData signature mismatch".to_string());
    }

    // Freshness: Telegram stamps auth_date; refuse day-old payloads.
    let auth_date = pairs
        .iter()
        .find(|(k, _)| k == "auth_date")
        .and_then(|(_, v)| v.parse::<i64>().ok())
        .ok_or("initData has no auth_date")?;
    if chrono::Utc::now().timestamp() - auth_date > 24 * 3600 {
        return Err("initData expired".to_string());
    }

    let user_json = pairs
        .iter()
        .find(|(k, _)| k == "user")
        .map(|(_, v)| v.clone())
        .ok_or("initData has no user")?;
    let user: Value = serde_json::from_str(&user_json).map_err(|e| e.to_string())?;
    user["id"].as_i64().ok_or("user has no id".to_string())
}

async fn authenticate(state: &WebState, headers: &HeaderMap) -> Result<Authed, ApiError> {
    let raw = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "missing Authorization header"))?;

    let user = if let Some(init_data) = raw.strip_prefix("tma ") {
        let telegram_id = verify_init_data(init_data, &state.bot_token).map_err(|e| {
            tracing::info!(event = %Event::WebRequestDenied, error = %e, "initData rejected");
            err(StatusCode::UNAUTHORIZED, e)
        })?;
        let telegram_id = TelegramId::from(telegram_id);
        match state
            .app
            .users
            .find_by_telegram_id(telegram_id)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?
        {
            Some(user) => user,
            // First contact through the app: register like /suggest would.
            None => state
                .app
                .users
                .create(telegram_id, Role::User, None, None)
                .await
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?,
        }
    } else if let Some(token) = raw.strip_prefix("Bearer ") {
        state
            .app
            .users
            .find_by_api_token(token.trim())
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?
            .ok_or_else(|| {
                tracing::info!(event = %Event::WebRequestDenied, "unknown bearer token");
                err(
                    StatusCode::UNAUTHORIZED,
                    "unknown token — run /apitoken again",
                )
            })?
    } else {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "use `tma <initData>` or `Bearer <token>`",
        ));
    };

    if user.is_banned {
        return Err(err(StatusCode::FORBIDDEN, "banned"));
    }
    Ok(Authed { user })
}

fn require(authed: &Authed, at_least: Role) -> Result<(), ApiError> {
    if authed.user.role >= at_least {
        Ok(())
    } else {
        Err(err(StatusCode::FORBIDDEN, format!("requires {at_least}")))
    }
}

// ------------------------------------------------------------- helpers ----

fn media_json(media: &ResolvedMedia) -> Value {
    match media {
        ResolvedMedia::Photo(url) => json!({ "kind": "photo", "url": url.as_str() }),
        ResolvedMedia::Video(url) => json!({ "kind": "video", "url": url.as_str() }),
        ResolvedMedia::Animation(url) => json!({ "kind": "animation", "url": url.as_str() }),
        ResolvedMedia::Link(url) => json!({ "kind": "link", "url": url.as_str() }),
        ResolvedMedia::TelegramCopy { .. } => json!({ "kind": "telegram_copy" }),
    }
}

fn tags_json(tags: &[Tag]) -> Vec<String> {
    tags.iter().map(ToString::to_string).collect()
}

fn parse_tags(list: &[String]) -> Vec<Tag> {
    list.iter()
        .flat_map(|entry| entry.split_whitespace())
        .map(Tag::from)
        .collect()
}

fn user_json(user: &Option<User>) -> Value {
    match user {
        None => Value::Null,
        Some(user) => json!({
            "id": user.id.as_ref(),
            "telegram_id": user.telegram_id.as_ref(),
            "name": user.display_name,
            "role": user.role.to_string(),
            "banned": user.is_banned,
        }),
    }
}

// ------------------------------------------------------------ handlers ----

async fn me(State(state): State<Arc<WebState>>, headers: HeaderMap) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    Ok(Json(json!({
        "telegram_id": authed.user.telegram_id.as_ref(),
        "name": authed.user.display_name,
        "role": authed.user.role.to_string(),
        "version": crate::state::APP_VERSION,
    })))
}

/// The moderation deck: everything awaiting review, oldest first.
async fn queue(State(state): State<Arc<WebState>>, headers: HeaderMap) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let posts = state
        .app
        .posts
        .list_by_status(PostStatus::AwaitingModeration)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut cards = Vec::new();
    for post in posts {
        let submitter = match post.submitted_by {
            None => None,
            Some(id) => state.app.users.find_by_id(id).await.ok().flatten(),
        };
        cards.push(json!({
            "post_id": post.id.as_ref(),
            "source": post.source.as_ref().as_str(),
            "tags": tags_json(&post.tags),
            "artists": tags_json(&post.artists),
            "submitted_at": post.submitted_at.to_rfc3339(),
            "submitter": user_json(&submitter),
        }));
    }
    Ok(Json(json!({ "cards": cards })))
}

/// Card media, resolved lazily (e621 pacing makes eager resolution of a
/// whole deck too slow).
async fn post_media(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    AxumPath(post_id): AxumPath<u64>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let post = state
        .app
        .posts
        .find_by_id(PostId::from(post_id))
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such post"))?;
    match state.app.resolver.resolve(&post.source).await {
        Ok(media) => Ok(Json(media_json(&media))),
        Err(e) => Ok(Json(
            json!({ "kind": "unavailable", "detail": e.to_string() }),
        )),
    }
}

#[derive(Deserialize)]
struct ModerateBody {
    post_id: u64,
    /// "approve" | "reject" | "changes"
    action: String,
    /// Extra tags to merge on approve (the 🏷 flow).
    #[serde(default)]
    extra_tags: Vec<String>,
    /// Reason to relay on reject (the 📝 flow), or the change list to relay
    /// on changes (the ✏️ flow — required there).
    #[serde(default)]
    reason: String,
}

async fn moderate_post(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<ModerateBody>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let actor = authed.user.telegram_id;
    let post_id = PostId::from(body.post_id);
    let message = match body.action.as_str() {
        "approve" => {
            let extra = parse_tags(&body.extra_tags);
            let result = if extra.is_empty() {
                moderate::approve(
                    ModerateCommand { actor, post_id },
                    &state.app.users,
                    &state.app.posts,
                )
                .await
            } else {
                moderate::approve_with_extra_tags(
                    ModerateCommand { actor, post_id },
                    extra,
                    &state.app.users,
                    &state.app.posts,
                )
                .await
            };
            match result {
                Err(e) => return Err(bad_request(e)),
                Ok(post) => {
                    notify_submitter_approved(&state.bot, &state.app, &post).await;
                    format!("Post #{post_id} accepted into the feed.")
                }
            }
        }
        "reject" if body.reason.trim().is_empty() => {
            match moderate::reject(
                ModerateCommand { actor, post_id },
                &state.app.users,
                &state.app.posts,
            )
            .await
            {
                Err(e) => return Err(bad_request(e)),
                Ok(_) => format!("Post #{post_id} rejected."),
            }
        }
        "reject" => {
            reject_with_reason(&state.bot, &state.app, actor, post_id, body.reason.trim()).await
        }
        "changes" if body.reason.trim().is_empty() => {
            return Err(bad_request("`changes` needs a message for the submitter"));
        }
        "changes" => {
            request_changes_with_message(&state.bot, &state.app, actor, post_id, body.reason.trim())
                .await
        }
        other => return Err(bad_request(format!("unknown action `{other}`"))),
    };
    Ok(Json(json!({ "message": message })))
}

// ---------------------------------------------------------------- feed ----

/// Queue overview: the feed end plus each poster's cursor distance — how
/// much curated backlog every channel still has ahead of it.
async fn feed_queue(State(state): State<Arc<WebState>>, headers: HeaderMap) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let feed_end = state
        .app
        .posts
        .feed_end()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let posters = state
        .app
        .posters
        .list_all()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut out = Vec::with_capacity(posters.len());
    for poster in posters {
        let config = state
            .app
            .publisher_configs
            .find_by_poster(poster.id)
            .await
            .ok()
            .flatten();
        out.push(json!({
            "id": poster.id.as_ref(),
            "chat_id": config.as_ref().map(|c| c.chat_id),
            "interval": poster.time_interval.as_ref(),
            "cursor": poster.cursor,
            "behind": feed_end.saturating_sub(poster.cursor),
            "subscribed": poster.subscribed_tags.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "subscribed_pretty": domain::elements::tag_rule::TagTerm::describe_list(&poster.subscribed_tags),
        }));
    }
    Ok(Json(json!({ "feed_end": feed_end, "posters": out })))
}

/// The feed after a post — everything still ahead of it, in feed order.
/// `token` is a post id or the #CODE from a published caption.
async fn feed_after(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    AxumPath(token): AxumPath<String>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let post_id = match crate::commands::parse_post_id(&token) {
        Some(id) => Some(id),
        None => resolve_publish_code(&state.app, &token).await,
    };
    let Some(post_id) = post_id else {
        return Err(err(StatusCode::NOT_FOUND, "no post with that id or code"));
    };
    let slice = feed::after_post(
        authed.user.telegram_id,
        post_id,
        &state.app.users,
        &state.app.posts,
    )
    .await
    .map_err(bad_request)?;
    Ok(Json(json!({
        "anchor": {
            "post_id": slice.anchor.id.as_ref(),
            "feed_position": slice.anchor.feed_position,
        },
        "feed_end": slice.feed_end,
        "entries": slice.entries.iter().map(|post| json!({
            "post_id": post.id.as_ref(),
            "feed_position": post.feed_position,
            "status": post.status.to_string(),
            "source": post.source.as_ref().as_str(),
            "tags": tags_json(&post.tags),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct QueuePageParams {
    /// Resume position from the previous page's `next_after`.
    #[serde(default)]
    after: Option<u64>,
    #[serde(default = "twenty")]
    limit: usize,
}
fn twenty() -> usize {
    20
}

/// One page of a poster's upcoming queue — only entries it WOULD post.
async fn poster_queue(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<u64>,
    Query(params): Query<QueuePageParams>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let queue = feed::poster_queue(
        authed.user.telegram_id,
        PosterId::from(id),
        params.after,
        params.limit.min(50),
        &state.app.users,
        &state.app.posters,
        &state.app.posts,
    )
    .await
    .map_err(bad_request)?;
    Ok(Json(json!({
        "poster_id": queue.poster.id.as_ref(),
        "cursor": queue.poster.cursor,
        "feed_end": queue.feed_end,
        "next_after": queue.next_after,
        "entries": queue.entries.iter().map(|post| json!({
            "post_id": post.id.as_ref(),
            "feed_position": post.feed_position,
            "source": post.source.as_ref().as_str(),
            "tags": tags_json(&post.tags),
        })).collect::<Vec<_>>(),
    })))
}

/// Remove a post from the feed — a global soft-delete (every consumer
/// skips it), not a per-poster hide.
async fn delete_post(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<u64>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let post_id = PostId::from(id);
    moderate::delete(
        ModerateCommand {
            actor: authed.user.telegram_id,
            post_id,
        },
        &state.app.users,
        &state.app.posts,
    )
    .await
    .map_err(bad_request)?;
    Ok(Json(json!({
        "message": format!("Post #{post_id} removed from the feed.")
    })))
}

// ------------------------------------------------------------- reports ----

/// The moderation overview: reported posts with who reported and why.
async fn list_reports(State(state): State<Arc<WebState>>, headers: HeaderMap) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let overview = report::overview(
        authed.user.telegram_id,
        &state.app.users,
        &state.app.posts,
        &state.app.reports,
    )
    .await
    .map_err(bad_request)?;

    let mut cards = Vec::with_capacity(overview.len());
    for reported in overview {
        let mut reports = Vec::with_capacity(reported.reports.len());
        for r in reported.reports {
            let name = state
                .app
                .users
                .find_by_telegram_id(r.reporter)
                .await
                .ok()
                .flatten()
                .and_then(|u| u.display_name);
            reports.push(json!({
                "reporter_telegram_id": r.reporter.as_ref(),
                "reporter_name": name,
                "reporter_username": r.reporter_username,
                "reason": r.reason,
                "at": r.reported_at.to_rfc3339(),
            }));
        }
        cards.push(json!({
            "post_id": reported.post.id.as_ref(),
            "source": reported.post.source.as_ref().as_str(),
            "status": reported.post.status.to_string(),
            "tags": tags_json(&reported.post.tags),
            "report_count": reports.len(),
            "reports": reports,
        }));
    }
    Ok(Json(json!({ "cards": cards })))
}

#[derive(Deserialize)]
struct ResolveReportBody {
    post_id: u64,
    /// "takedown" | "dismiss"
    action: String,
}

async fn resolve_report(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<ResolveReportBody>,
) -> ApiResult {
    use teloxide::prelude::Requester as _;
    use teloxide::types::{ChatId, MessageId};

    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let actor = authed.user.telegram_id;
    let post_id = PostId::from(body.post_id);
    let message = match body.action.as_str() {
        "dismiss" => {
            report::dismiss(actor, post_id, &state.app.users, &state.app.reports)
                .await
                .map_err(bad_request)?;
            format!("Reports for post #{post_id} dismissed.")
        }
        "takedown" => {
            let deliveries = report::take_down(
                actor,
                post_id,
                &state.app.users,
                &state.app.posts,
                &state.app.publications,
            )
            .await
            .map_err(bad_request)?;
            let mut deleted = 0usize;
            for delivery in &deliveries {
                match state
                    .bot
                    .delete_message(ChatId(delivery.chat_id), MessageId(delivery.message_id))
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
        other => return Err(bad_request(format!("unknown action `{other}`"))),
    };
    Ok(Json(json!({ "message": message })))
}

#[derive(Deserialize)]
struct BrowseParams {
    #[serde(default)]
    tags: String,
    #[serde(default = "one")]
    page: u32,
    #[serde(default = "ten")]
    count: usize,
}
fn one() -> u32 {
    1
}
fn ten() -> usize {
    10
}

async fn browse_e621(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Query(params): Query<BrowseParams>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let mut results = browse::search(
        browse::BrowseCommand {
            actor: authed.user.telegram_id,
            tags: params.tags.split_whitespace().map(Tag::from).collect(),
            page: params.page,
        },
        &state.app.users,
        &*state.app.e621,
        &state.app.forbidden,
        &state.app.required,
        &state.app.posts,
        &state.app.skips,
    )
    .await
    .map_err(bad_request)?;
    results.truncate(params.count.min(20));
    let cards: Vec<Value> = results
        .iter()
        .map(|m| {
            json!({
                "source": m.source.as_ref().as_str(),
                "tags": tags_json(&m.tags),
                "artists": tags_json(&m.artists),
                "preview_url": m.preview_url.as_str(),
                "file_url": m.file_url.as_str(),
                "mp4_url": m.mp4_url.as_ref().map(|u| u.as_str().to_string()),
            })
        })
        .collect();
    Ok(Json(json!({ "cards": cards, "page": params.page })))
}

#[derive(Deserialize)]
struct SkipBody {
    url: String,
}

/// Skip a browse result for good: dedupe can't catch video re-uploads, so
/// the verdict is remembered and browse never shows the source again.
async fn skip_post(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<SkipBody>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let url = Url::parse(&body.url).map_err(bad_request)?;
    let source = browse::skip(authed.user.telegram_id, url, &state.app.users, &state.app.skips)
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({
        "message": format!("Skipped for good — {} won't show in browse again.", source.as_ref())
    })))
}

#[derive(Deserialize)]
struct SaveBody {
    url: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn save_post(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<SaveBody>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let url = Url::parse(&body.url).map_err(bad_request)?;
    match browse::save(
        browse::SaveCommand {
            actor: authed.user.telegram_id,
            url,
            tags: parse_tags(&body.tags),
        },
        &state.app.users,
        &state.app.posts,
        &*state.app.e621,
        &state.app.forbidden,
    )
    .await
    .map_err(bad_request)?
    {
        browse::SaveOutcome::TagsNeeded => {
            Err(err(StatusCode::UNPROCESSABLE_ENTITY, "tags_needed"))
        }
        browse::SaveOutcome::Added(post) => Ok(Json(json!({
            "message": format!("Post #{} entered the feed.", post.id),
            "post_id": post.id.as_ref(),
        }))),
    }
}

#[derive(Deserialize)]
struct ResolveBody {
    url: String,
}

/// Submission preview: validate the source, flag duplicates, resolve the
/// media, and pre-fetch e621 tags so the user confirms what they saw.
async fn resolve_preview(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<ResolveBody>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    let url = Url::parse(&body.url).map_err(bad_request)?;
    let source = Source::try_from(url).map_err(bad_request)?;
    let duplicate = state
        .app
        .posts
        .find_by_source(&source)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?
        .map(|p| *p.id.as_ref());
    let media = match state.app.resolver.resolve(&source).await {
        Ok(media) => media_json(&media),
        Err(e) => json!({ "kind": "unavailable", "detail": e.to_string() }),
    };
    let (tags, artists, needs_tags) = match &source {
        Source::E621(_) => match state.app.e621.fetch(&source).await {
            Ok(m) => (tags_json(&m.tags), tags_json(&m.artists), false),
            Err(e) => return Err(bad_request(e)),
        },
        _ => (vec![], vec![], true),
    };
    let _ = &authed;
    Ok(Json(json!({
        "source": source.as_ref().as_str(),
        "duplicate_of": duplicate,
        "media": media,
        "tags": tags,
        "artists": artists,
        "needs_tags": needs_tags,
    })))
}

#[derive(Deserialize)]
struct SuggestBody {
    url: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn suggest_post(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<SuggestBody>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    let url = Url::parse(&body.url).map_err(bad_request)?;
    let tags = parse_tags(&body.tags);

    // Curators don't review themselves: Moderator+ submissions take the
    // /save path — straight into the feed, no queue, no "Submitted by"
    // (admin adds carry no submitter attribution).
    if authed.user.role >= Role::Moderator {
        return match browse::save(
            browse::SaveCommand {
                actor: authed.user.telegram_id,
                url,
                tags,
            },
            &state.app.users,
            &state.app.posts,
            &*state.app.e621,
            &state.app.forbidden,
        )
        .await
        .map_err(bad_request)?
        {
            browse::SaveOutcome::TagsNeeded => {
                Err(err(StatusCode::UNPROCESSABLE_ENTITY, "tags_needed"))
            }
            browse::SaveOutcome::Added(post) => Ok(Json(json!({
                "message": format!("Saved straight into the feed as post #{} — no review needed.", post.id),
                "post_id": post.id.as_ref(),
            }))),
        };
    }

    let submitter = Submitter {
        id: authed.user.telegram_id,
        display_name: authed.user.display_name.clone(),
        username: None,
    };
    // The shared pipeline handles review-DM fan-out to moderators.
    let message = submit(&state.bot, &state.app, submitter, url, tags, None).await;
    Ok(Json(json!({ "message": message })))
}

// ------------------------------------------------------- owner: posters ---

async fn list_posters(State(state): State<Arc<WebState>>, headers: HeaderMap) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Owner)?;
    let posters = state
        .app
        .posters
        .list_all()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut out = Vec::new();
    for poster in posters {
        let config = state
            .app
            .publisher_configs
            .find_by_poster(poster.id)
            .await
            .ok()
            .flatten();
        out.push(json!({
            "id": poster.id.as_ref(),
            "interval": poster.time_interval.as_ref(),
            "cursor": poster.cursor,
            "subscribed": poster.subscribed_tags.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "subscribed_pretty": domain::elements::tag_rule::TagTerm::describe_list(&poster.subscribed_tags),
            "forbidden": tags_json(&poster.forbidden_tags),
            "rules": poster.rules.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "rules_pretty": poster.rules.iter().map(|r| r.describe()).collect::<Vec<_>>(),
            "chat_id": config.as_ref().map(|c| c.chat_id),
            "announcements": config.as_ref().map(|c| c.receive_announcements),
            "summary": poster_summary(&state.app, &poster, "").await,
        }));
    }
    Ok(Json(json!({ "posters": out })))
}

#[derive(Deserialize)]
struct NewPosterBody {
    interval: u8,
    chat: String,
    #[serde(default)]
    tags: String,
}

async fn create_poster(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<NewPosterBody>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Owner)?;
    let interval =
        domain::elements::cadence::PostInterval::new(body.interval).map_err(bad_request)?;
    let chat_id = resolve_chat(&state, &body.chat).await?;
    let (subscribed, forbidden) =
        crate::commands::parse_tag_lists(body.tags.split_whitespace()).map_err(bad_request)?;
    let poster = manage_poster::new_poster(
        manage_poster::NewPoster {
            actor: authed.user.telegram_id,
            subscribed_tags: subscribed,
            forbidden_tags: forbidden,
            interval,
            chat_id,
            token_path: state.app.config.token_path(),
        },
        &state.app.users,
        &state.app.posters,
        &state.app.publisher_configs,
        &state.app.posts,
    )
    .await
    .map_err(bad_request)?;
    Ok(Json(json!({
        "message": poster_summary(&state.app, &poster, "created, live within a minute").await
    })))
}

async fn resolve_chat(state: &WebState, raw: &str) -> Result<i64, ApiError> {
    if let Ok(id) = raw.parse::<i64>() {
        return Ok(id);
    }
    let resolver = crate::resolvers::BotUserResolver {
        bot: state.bot.clone(),
    };
    match crate::commands::resolve_target(&resolver, raw).await {
        Ok(Some(id)) => Ok(*id.as_ref()),
        Ok(None) => Err(bad_request(format!(
            "can't resolve {raw} — is the bot in that channel?"
        ))),
        Err(e) => Err(bad_request(e)),
    }
}

#[derive(Deserialize)]
struct PatchPosterBody {
    /// Full-replace subscription+forbidden ("" clears). None = untouched.
    tags: Option<String>,
    /// Full-replace rules ("" clears). None = untouched.
    rules: Option<String>,
    interval: Option<u8>,
    chat: Option<String>,
    announcements: Option<bool>,
}

async fn patch_poster(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<u64>,
    Json(body): Json<PatchPosterBody>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Owner)?;
    let actor = authed.user.telegram_id;
    let poster_id = PosterId::from(id);

    if let Some(tags) = &body.tags {
        let (subscribed, forbidden) =
            crate::commands::parse_tag_lists(tags.split_whitespace()).map_err(bad_request)?;
        manage_poster::set_tags(
            manage_poster::SetTags {
                actor,
                poster_id,
                subscribed_tags: subscribed,
                forbidden_tags: forbidden,
            },
            &state.app.users,
            &state.app.posters,
        )
        .await
        .map_err(bad_request)?;
    }
    if let Some(rules) = &body.rules {
        let rules = TagRule::parse_all(rules).map_err(bad_request)?;
        manage_poster::set_rules(
            actor,
            poster_id,
            rules,
            &state.app.users,
            &state.app.posters,
        )
        .await
        .map_err(bad_request)?;
    }
    if let Some(minutes) = body.interval {
        let interval =
            domain::elements::cadence::PostInterval::new(minutes).map_err(bad_request)?;
        manage_poster::set_interval(
            actor,
            poster_id,
            interval,
            &state.app.users,
            &state.app.posters,
        )
        .await
        .map_err(bad_request)?;
    }
    if let Some(chat) = &body.chat {
        let chat_id = resolve_chat(&state, chat).await?;
        manage_poster::set_channel(
            manage_poster::SetChannel {
                actor,
                poster_id,
                chat_id,
                token_path: state.app.config.token_path(),
            },
            &state.app.users,
            &state.app.posters,
            &state.app.publisher_configs,
        )
        .await
        .map_err(bad_request)?;
    }
    if let Some(receive) = body.announcements {
        let chat_id = state
            .app
            .publisher_configs
            .find_by_poster(poster_id)
            .await
            .ok()
            .flatten()
            .map(|c| c.chat_id)
            .ok_or_else(|| bad_request("poster has no channel yet"))?;
        manage_poster::set_announcement_mute(
            actor,
            chat_id,
            !receive,
            &state.app.users,
            &state.app.publisher_configs,
        )
        .await
        .map_err(bad_request)?;
    }

    let poster = state
        .app
        .posters
        .find_by_id(poster_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such poster"))?;
    Ok(Json(json!({
        "message": poster_summary(&state.app, &poster, "updated, live within a minute").await
    })))
}

async fn delete_poster(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<u64>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Owner)?;
    manage_poster::delete_poster(
        authed.user.telegram_id,
        PosterId::from(id),
        &state.app.users,
        &state.app.posters,
        &state.app.publisher_configs,
    )
    .await
    .map_err(bad_request)?;
    Ok(Json(json!({ "message": format!("Poster #{id} deleted.") })))
}

// ------------------------------------------------- owner: tag policies ----

async fn list_tag_policies(State(state): State<Arc<WebState>>, headers: HeaderMap) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let forbidden = state
        .app
        .forbidden
        .list_all()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let required = state
        .app
        .required
        .list_all()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let spoilers = state
        .app
        .spoilers
        .list_all()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let forbidden_reasons = state
        .app
        .forbidden
        .list_with_reasons()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?
        .into_iter()
        .map(|(tag, reason)| json!({ "tag": tag.to_string(), "reason": reason }))
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "forbidden": tags_json(&forbidden),
        "forbidden_with_reasons": forbidden_reasons,
        "required": tags_json(&required),
        "spoilers": tags_json(&spoilers),
    })))
}

#[derive(Deserialize)]
struct TagPolicyBody {
    /// "forbidden" | "required" | "spoilers"
    list: String,
    tag: String,
    /// true = add, false = remove
    add: bool,
    /// Why (forbidden adds only) — upserts on re-add.
    #[serde(default)]
    reason: Option<String>,
}

async fn edit_tag_policy(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<TagPolicyBody>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Owner)?;
    let tag = Tag::from(body.tag.trim());
    let result = match (body.list.as_str(), body.add) {
        ("forbidden", true) => state
            .app
            .forbidden
            .add(
                tag,
                body.reason
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty()),
            )
            .await
            .map_err(|e| e.to_string()),
        ("forbidden", false) => state
            .app
            .forbidden
            .remove(&tag)
            .await
            .map_err(|e| e.to_string()),
        ("required", true) => state.app.required.add(tag).await.map_err(|e| e.to_string()),
        ("required", false) => state
            .app
            .required
            .remove(&tag)
            .await
            .map_err(|e| e.to_string()),
        ("spoilers", true) => state.app.spoilers.add(tag).await.map_err(|e| e.to_string()),
        ("spoilers", false) => state
            .app
            .spoilers
            .remove(&tag)
            .await
            .map_err(|e| e.to_string()),
        _ => return Err(bad_request("list must be forbidden|required|spoilers")),
    };
    result.map_err(bad_request)?;
    Ok(Json(json!({ "message": "updated" })))
}

// -------------------------------------------------------- owner: users ----

#[derive(Deserialize)]
struct ShadowBanBody {
    telegram_id: i64,
    banned: bool,
}

/// Set or lift a shadowban: the target keeps the full report/wish/submit
/// flow with the same responses, but nothing they send ever lands.
async fn set_shadow_ban(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<ShadowBanBody>,
) -> ApiResult {
    use domain::elements::shadow_ban::ShadowBanRepository as _;

    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let who = TelegramId::from(body.telegram_id);
    if body.banned {
        state
            .app
            .shadow_bans
            .set(who, authed.user.telegram_id, chrono::Utc::now())
            .await
            .map_err(bad_request)?;
    } else {
        state.app.shadow_bans.lift(who).await.map_err(bad_request)?;
    }
    tracing::info!(
        event = %Event::ShadowBanChanged, who = body.telegram_id,
        banned = body.banned, by = authed.user.telegram_id.as_ref(),
        "shadow ban changed"
    );
    Ok(Json(json!({
        "message": if body.banned {
            format!("{} is shadowbanned — they'll never know.", body.telegram_id)
        } else {
            format!("Shadowban lifted for {}.", body.telegram_id)
        }
    })))
}

async fn list_users(State(state): State<Arc<WebState>>, headers: HeaderMap) -> ApiResult {
    use domain::elements::shadow_ban::ShadowBanRepository as _;

    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Owner)?;
    let mut users = Vec::new();
    for role in [Role::Owner, Role::Moderator, Role::User] {
        for user in state
            .app
            .users
            .list_by_role(role)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?
        {
            let shadow = state
                .app
                .shadow_bans
                .contains(user.telegram_id)
                .await
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
            let mut entry = user_json(&Some(user));
            entry["shadow_banned"] = json!(shadow);
            users.push(entry);
        }
    }
    Ok(Json(json!({ "users": users })))
}

#[derive(Deserialize)]
struct ProfileParams {
    #[serde(default)]
    offset: u32,
}

/// One user's profile: identity, ban states, submission stats, and a page
/// of their submitted artwork (newest first).
async fn user_profile(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<u64>,
    Query(params): Query<ProfileParams>,
) -> ApiResult {
    use domain::elements::shadow_ban::ShadowBanRepository as _;
    use domain::elements::user::UserId;

    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Owner)?;
    let user = state
        .app
        .users
        .find_by_id(UserId::from(id))
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such user"))?;
    let shadow = state
        .app
        .shadow_bans
        .contains(user.telegram_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let counts = state
        .app
        .posts
        .count_by_submitter(user.id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let total: u64 = counts.iter().map(|(_, n)| n).sum();
    let submissions = state
        .app
        .posts
        .list_by_submitter(user.id, 20, params.offset)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let mut entry = user_json(&Some(user));
    entry["shadow_banned"] = json!(shadow);
    Ok(Json(json!({
        "user": entry,
        "stats": {
            "total": total,
            "by_status": counts.iter().map(|(status, n)| json!({
                "status": status.to_string(),
                "count": n,
            })).collect::<Vec<_>>(),
        },
        "submissions": submissions.iter().map(|post| json!({
            "post_id": post.id.as_ref(),
            "status": post.status.to_string(),
            "source": post.source.as_ref().as_str(),
            "submitted_at": post.submitted_at.to_rfc3339(),
            "feed_position": post.feed_position,
        })).collect::<Vec<_>>(),
        "next_offset": if submissions.len() == 20 {
            json!(params.offset + 20)
        } else {
            json!(null)
        },
    })))
}

#[derive(Deserialize)]
struct PatchUserBody {
    role: Option<String>,
    banned: Option<bool>,
}

async fn patch_user(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    AxumPath(user_id): AxumPath<u64>,
    Json(body): Json<PatchUserBody>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Owner)?;
    let id = domain::elements::user::UserId::from(user_id);
    if let Some(role) = &body.role {
        let role: Role = role.parse().map_err(bad_request)?;
        state
            .app
            .users
            .change_role(id, role)
            .await
            .map_err(bad_request)?;
    }
    if let Some(banned) = body.banned {
        state
            .app
            .users
            .set_banned(id, banned)
            .await
            .map_err(bad_request)?;
    }
    Ok(Json(json!({ "message": "updated" })))
}

// ------------------------------------------------------------ postinfo ----

async fn postinfo(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    AxumPath(token): AxumPath<String>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let post_id = match crate::commands::parse_post_id(&token) {
        Some(id) => Some(id),
        None => resolve_publish_code(&state.app, &token).await,
    };
    let Some(post_id) = post_id else {
        return Err(err(StatusCode::NOT_FOUND, "no post with that id or code"));
    };
    let info = post_info(
        authed.user.telegram_id,
        post_id,
        &state.app.users,
        &state.app.posts,
        &state.app.publications,
        &state.app.reports,
    )
    .await
    .map_err(bad_request)?;
    let verdicts = poster_verdicts(&state.app, &info.post, &info.publications).await;
    let post = &info.post;
    Ok(Json(json!({
        "post_id": post.id.as_ref(),
        "status": post.status.to_string(),
        "feed_position": post.feed_position,
        "source": post.source.as_ref().as_str(),
        "tags": tags_json(&post.tags),
        "artists": tags_json(&post.artists),
        "submitted_at": post.submitted_at.to_rfc3339(),
        "submitter": user_json(&info.submitter),
        "moderated_at": post.moderated_at.map(|at| at.to_rfc3339()),
        "moderator": user_json(&info.moderator),
        "last_posted": post.last_posted.map(|at| at.to_rfc3339()),
        "report_count": info.report_count,
        "publications": info.publications.iter().map(|p| json!({
            "chat_id": p.chat_id,
            "message_id": p.message_id,
            "at": p.published_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
        "verdicts": verdicts,
    })))
}

/// Eligibility preview used by the submit screen: which posters would take
/// a post with these tags.
async fn eligibility(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<EligibilityBody>,
) -> ApiResult {
    let authed = authenticate(&state, &headers).await?;
    require(&authed, Role::Moderator)?;
    let tags: std::collections::HashSet<Tag> = parse_tags(&body.tags).into_iter().collect();
    let posters = state
        .app
        .posters
        .list_all()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let out: Vec<Value> = posters
        .iter()
        .map(|poster| {
            json!({
                "poster_id": poster.id.as_ref(),
                "refusal": refusal_for(poster, &tags).map(|r| r.to_string()),
            })
        })
        .collect();
    Ok(Json(json!({ "posters": out })))
}

#[derive(Deserialize)]
struct EligibilityBody {
    #[serde(default)]
    tags: Vec<String>,
}

// -------------------------------------------------------------- router ----

pub fn router(state: Arc<WebState>, webapp_dir: Option<std::path::PathBuf>) -> axum::Router {
    let api = axum::Router::new()
        .route("/me", get(me))
        .route("/queue", get(queue))
        .route("/posts/{id}/media", get(post_media))
        .route("/moderate", post(moderate_post))
        .route("/reports", get(list_reports))
        .route("/reports/resolve", post(resolve_report))
        .route("/feed/queue", get(feed_queue))
        .route("/feed/after/{token}", get(feed_after))
        .route("/posters/{id}/queue", get(poster_queue))
        .route("/posts/{id}", axum::routing::delete(delete_post))
        .route("/browse", get(browse_e621))
        .route("/save", post(save_post))
        .route("/browse/skip", post(skip_post))
        .route("/resolve", post(resolve_preview))
        .route("/suggest", post(suggest_post))
        .route("/eligibility", post(eligibility))
        .route("/posters", get(list_posters).post(create_poster))
        .route(
            "/posters/{id}",
            axum::routing::patch(patch_poster).delete(delete_poster),
        )
        .route(
            "/tag-policies",
            get(list_tag_policies).post(edit_tag_policy),
        )
        .route("/users", get(list_users))
        .route("/shadowban", post(set_shadow_ban))
        .route("/users/{id}", axum::routing::patch(patch_user))
        .route("/users/{id}/profile", get(user_profile))
        .route("/postinfo/{token}", get(postinfo))
        .with_state(state);

    let mut router = axum::Router::new().nest("/api", api);
    if let Some(dir) = webapp_dir {
        let serve = tower_http::services::ServeDir::new(&dir)
            .fallback(tower_http::services::ServeFile::new(dir.join("index.html")));
        router = router.fallback_service(serve);
    }
    router
}
