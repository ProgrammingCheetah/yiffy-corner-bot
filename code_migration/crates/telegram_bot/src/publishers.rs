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
}

impl TelegramPublisher {
    pub fn new(bot: Bot, chat_id: ChatId) -> Self {
        Self { bot, chat_id }
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
        let message_id = match &item.media {
            ResolvedMedia::Photo(url) => {
                let mut request = self
                    .bot
                    .send_photo(self.chat_id, InputFile::url(url.clone()));
                if let Some(caption) = &item.caption {
                    request = request.caption(caption.clone()).parse_mode(ParseMode::Html);
                }
                request.await.map_err(send)?.id
            }
            ResolvedMedia::Video(url) => {
                let mut request = self
                    .bot
                    .send_video(self.chat_id, InputFile::url(url.clone()));
                if let Some(caption) = &item.caption {
                    request = request.caption(caption.clone()).parse_mode(ParseMode::Html);
                }
                request.await.map_err(send)?.id
            }
            ResolvedMedia::Animation(url) => {
                let mut request = self
                    .bot
                    .send_animation(self.chat_id, InputFile::url(url.clone()));
                if let Some(caption) = &item.caption {
                    request = request.caption(caption.clone()).parse_mode(ParseMode::Html);
                }
                request.await.map_err(send)?.id
            }
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
                let text = item.caption.clone().unwrap_or_else(|| url.to_string());
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
                    .map_err(send)?
                    .id
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
}

impl DbPublisherFactory {
    pub fn new(
        configs: SqlitePublisherConfigRepository,
        main_bot: Bot,
        main_token: String,
    ) -> Self {
        Self {
            configs,
            main_bot,
            main_token,
            bots: tokio::sync::Mutex::new(HashMap::new()),
        }
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
        Ok(Some(Box::new(TelegramPublisher::new(
            bot,
            ChatId(config.chat_id),
        ))))
    }
}
