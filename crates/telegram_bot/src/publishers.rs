use std::collections::HashMap;

use async_trait::async_trait;
use domain::elements::{
    media::ResolvedMedia,
    poster::Poster,
    publisher::{PublishItem, PublishReceipt, Publisher, PublisherError},
    publisher_config::PublisherConfigRepository as _,
};
use persistence::sqlite::publisher_config::SqlitePublisherConfigRepository;
use telemetry::{Event, Upstream};
use teloxide::{
    Bot,
    payloads::{
        CopyMessageSetters, SendAnimationSetters, SendMessageSetters, SendPhotoSetters,
        SendVideoSetters,
    },
    prelude::Requester,
    types::{ChatId, InputFile, LinkPreviewOptions, MessageId, ParseMode},
};

use crate::state::read_secret;

/// Publishes resolved media to a Telegram chat, dispatching on the media
/// kind: photo/video/animation as native media messages with the caption,
/// links as a text message that leans on Telegram's link preview (this is
/// how FixUp embeds and t.me sources render). Captions are HTML (code
/// header, Source/Report hyperlinks — no bulky buttons); the returned
/// receipt records where the message landed.
pub struct TelegramPublisher {
    bot: Bot,
    chat_id: ChatId,
    /// The channel's public @handle (without `@`), appended as the caption's
    /// last line on every publish — channel self-branding.
    channel_handle: Option<String>,
    /// Fetches media bytes ourselves when Telegram's server-side URL fetch
    /// is refused (e621's CDN turns its fetcher away now and then).
    http: reqwest::Client,
}

/// Telegram's upload cap for photos…
const PHOTO_BYTE_CAP: u64 = 10 * 1024 * 1024;
/// …and for everything else a bot can upload.
const MEDIA_BYTE_CAP: u64 = 50 * 1024 * 1024;

/// Which native media message a publish maps to.
#[derive(Clone, Copy)]
enum MediaKind {
    Photo,
    Video,
    Animation,
}

impl MediaKind {
    fn byte_cap(self) -> u64 {
        match self {
            MediaKind::Photo => PHOTO_BYTE_CAP,
            _ => MEDIA_BYTE_CAP,
        }
    }
}

impl TelegramPublisher {
    pub fn new(
        bot: Bot,
        chat_id: ChatId,
        channel_handle: Option<String>,
        http: reqwest::Client,
    ) -> Self {
        Self {
            bot,
            chat_id,
            channel_handle,
            http,
        }
    }

    /// Link-embed send: HTML caption as the text, preview forced onto `url`.
    /// The publish path for Link media AND the graceful fallback when
    /// Telegram refuses to fetch a direct media URL (too big, webm, …).
    async fn send_link(
        &self,
        caption: Option<&str>,
        url: &url::Url,
    ) -> Result<MessageId, teloxide::RequestError> {
        let text = caption
            .map(ToString::to_string)
            .unwrap_or_else(|| url.to_string());
        self.bot
            .send_message(self.chat_id, text)
            .parse_mode(ParseMode::Html)
            .link_preview_options(LinkPreviewOptions {
                is_disabled: false,
                url: Some(url.to_string()),
                prefer_small_media: false,
                prefer_large_media: true,
                show_above_text: false,
            })
            .await
            .map(|message| message.id)
    }

    /// One native media send, dispatched on kind.
    async fn send_media(
        &self,
        kind: MediaKind,
        input: InputFile,
        caption: Option<&str>,
        spoiler: bool,
    ) -> Result<MessageId, teloxide::RequestError> {
        match kind {
            MediaKind::Photo => {
                let mut request = self.bot.send_photo(self.chat_id, input).has_spoiler(spoiler);
                if let Some(caption) = caption {
                    request = request
                        .caption(caption.to_owned())
                        .parse_mode(ParseMode::Html);
                }
                request.await.map(|message| message.id)
            }
            MediaKind::Video => {
                let mut request = self.bot.send_video(self.chat_id, input).has_spoiler(spoiler);
                if let Some(caption) = caption {
                    request = request
                        .caption(caption.to_owned())
                        .parse_mode(ParseMode::Html);
                }
                request.await.map(|message| message.id)
            }
            MediaKind::Animation => {
                let mut request = self
                    .bot
                    .send_animation(self.chat_id, input)
                    .has_spoiler(spoiler);
                if let Some(caption) = caption {
                    request = request
                        .caption(caption.to_owned())
                        .parse_mode(ParseMode::Html);
                }
                request.await.map(|message| message.id)
            }
        }
    }

    /// Fetch the media ourselves and upload the bytes — the recovery when
    /// Telegram won't fetch the URL server-side.
    async fn reupload(
        &self,
        kind: MediaKind,
        url: &url::Url,
        caption: Option<&str>,
        spoiler: bool,
    ) -> Result<MessageId, String> {
        let response = self
            .http
            .get(url.clone())
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| e.to_string())?;
        if let Some(length) = response.content_length()
            && length > kind.byte_cap()
        {
            return Err(format!("{length} bytes exceeds the upload cap"));
        }
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() as u64 > kind.byte_cap() {
            return Err(format!("{} bytes exceeds the upload cap", bytes.len()));
        }
        // Telegram sniffs the container from the file name.
        let name = url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .filter(|segment| !segment.is_empty())
            .unwrap_or("media")
            .to_string();
        self.send_media(
            kind,
            InputFile::memory(bytes.to_vec()).file_name(name),
            caption,
            spoiler,
        )
        .await
        .map_err(|e| e.to_string())
    }

    /// URL send → bytes re-upload → link embed, in that order. Telegram's
    /// own fetch of e621 URLs fails intermittently ("failed to get HTTP URL
    /// content" / "wrong type of the web page content"), so a refusal is
    /// retried with the bytes before degrading to a bare link.
    async fn send_media_with_fallback(
        &self,
        kind: MediaKind,
        url: &url::Url,
        caption: Option<&str>,
        spoiler: bool,
    ) -> Result<MessageId, teloxide::RequestError> {
        let refused = match self
            .send_media(kind, InputFile::url(url.clone()), caption, spoiler)
            .await
        {
            Ok(message_id) => return Ok(message_id),
            Err(e) => e,
        };
        match self.reupload(kind, url, caption, spoiler).await {
            Ok(message_id) => {
                tracing::info!(
                    event = %Event::MediaReuploaded, upstream = %Upstream::Telegram,
                    url = %url, refusal = %refused,
                    "URL send refused; re-uploaded the bytes instead"
                );
                Ok(message_id)
            }
            Err(reupload_error) => {
                tracing::warn!(
                    event = %Event::MediaLinkFallback, upstream = %Upstream::Telegram,
                    url = %url, error = %refused, reupload_error,
                    "direct media send refused; falling back to link"
                );
                self.send_link(caption, url).await
            }
        }
    }
}

#[async_trait]
impl Publisher for TelegramPublisher {
    async fn publish(&self, item: &PublishItem) -> Result<PublishReceipt, PublisherError> {
        tracing::debug!(
            event = %Event::UpstreamRequest, upstream = %Upstream::Telegram,
            chat = self.chat_id.0, media = ?item.media, "sending publish message"
        );
        let send = |e: teloxide::RequestError| PublisherError::Send(e.to_string());
        // Channel self-branding: the @handle closes every caption.
        let caption = match (&item.caption, &self.channel_handle) {
            (Some(caption), Some(handle)) => Some(format!("{caption}\n\n@{handle}")),
            (None, Some(handle)) => Some(format!("@{handle}")),
            (caption, None) => caption.clone(),
        };
        let item = &PublishItem {
            caption,
            ..item.clone()
        };
        let message_id = match &item.media {
            ResolvedMedia::Photo(url) => self
                .send_media_with_fallback(MediaKind::Photo, url, item.caption.as_deref(), item.spoiler)
                .await
                .map_err(send)?,
            ResolvedMedia::Video(url) => self
                .send_media_with_fallback(MediaKind::Video, url, item.caption.as_deref(), item.spoiler)
                .await
                .map_err(send)?,
            ResolvedMedia::Animation(url) => self
                .send_media_with_fallback(
                    MediaKind::Animation,
                    url,
                    item.caption.as_deref(),
                    item.spoiler,
                )
                .await
                .map_err(send)?,
            ResolvedMedia::TelegramCopy {
                origin_chat_id,
                origin_message_id,
            } => {
                // Copy = content without the "Forwarded from" header; the
                // caption carries the channel attribution instead.
                let mut request = self.bot.copy_message(
                    self.chat_id,
                    ChatId(*origin_chat_id),
                    MessageId(*origin_message_id),
                );
                if let Some(caption) = &item.caption {
                    request = request.caption(caption.clone()).parse_mode(ParseMode::Html);
                }
                request.await.map_err(send)?
            }
            ResolvedMedia::Link(url) => {
                // Embed-URL publish: the caption's Source link points at the
                // original page, so force the preview onto the embed URL
                // (fixupx/fxbsky) — that's the whole point of the rewrite.
                self.send_link(item.caption.as_deref(), url)
                    .await
                    .map_err(send)?
            }
        };
        Ok(PublishReceipt {
            chat_id: self.chat_id.0,
            message_id: message_id.0,
        })
    }
}

/// Database-first delivery: looks a Poster's binding up at fire time, so
/// `/setchannel` is live on the next tick. Bots are cached per token; the
/// main bot is reused when a Poster publishes with the main token.
pub struct DbPublisherFactory {
    configs: SqlitePublisherConfigRepository,
    main_bot: Bot,
    main_token: String,
    bots: tokio::sync::Mutex<HashMap<String, Bot>>,
    /// chat id → resolved public @handle (None = private / no handle).
    /// Resolved once per chat per process.
    handles: tokio::sync::Mutex<HashMap<i64, Option<String>>>,
    /// Shared by every publisher for media byte re-uploads.
    http: reqwest::Client,
}

impl DbPublisherFactory {
    pub fn new(
        configs: SqlitePublisherConfigRepository,
        main_bot: Bot,
        main_token: String,
    ) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(crate::state::USER_AGENT)
            .connect_timeout(std::time::Duration::from_secs(10))
            // Generous: a re-upload may pull up to 50 MB off the CDN.
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest client");
        Self {
            configs,
            main_bot,
            main_token,
            bots: tokio::sync::Mutex::new(HashMap::new()),
            handles: tokio::sync::Mutex::new(HashMap::new()),
            http,
        }
    }

    async fn channel_handle(&self, bot: &Bot, chat_id: i64) -> Option<String> {
        if let Some(cached) = self.handles.lock().await.get(&chat_id) {
            return cached.clone();
        }
        let handle = match bot.get_chat(ChatId(chat_id)).await {
            Ok(chat) => chat.username().map(ToString::to_string),
            Err(e) => {
                tracing::debug!(chat_id, error = %e, "channel handle resolution failed");
                return None; // not cached: retry next fire
            }
        };
        self.handles.lock().await.insert(chat_id, handle.clone());
        handle
    }
}

#[async_trait]
impl application::actors::scheduler::PublisherFactory for DbPublisherFactory {
    async fn publisher_for(&self, poster: &Poster) -> Result<Option<Box<dyn Publisher>>, String> {
        let Some(config) = self
            .configs
            .find_by_poster(poster.id)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        let token = read_secret(&config.token_path).map_err(|e| e.to_string())?;
        let bot = if token == self.main_token {
            self.main_bot.clone()
        } else {
            let mut bots = self.bots.lock().await;
            bots.entry(token.clone())
                .or_insert_with(|| Bot::new(token))
                .clone()
        };
        let handle = self.channel_handle(&bot, config.chat_id).await;
        Ok(Some(Box::new(TelegramPublisher::new(
            bot,
            ChatId(config.chat_id),
            handle,
            self.http.clone(),
        ))))
    }
}
