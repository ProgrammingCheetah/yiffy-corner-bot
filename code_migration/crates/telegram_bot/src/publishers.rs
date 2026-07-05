use async_trait::async_trait;
use domain::elements::{
    media::ResolvedMedia,
    publisher::{PublishItem, Publisher, PublisherError},
};
use teloxide::{
    Bot,
    payloads::{SendAnimationSetters, SendPhotoSetters, SendVideoSetters},
    prelude::Requester,
    types::{ChatId, InputFile},
};

/// Publishes resolved media to a Telegram chat, dispatching on the media
/// kind: photo/video/animation as native media messages with the caption,
/// links as a text message that leans on Telegram's link preview (this is
/// how FixUp embeds and t.me sources render).
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
    async fn publish(&self, item: &PublishItem) -> Result<(), PublisherError> {
        let send = |e: teloxide::RequestError| PublisherError::Send(e.to_string());
        match &item.media {
            ResolvedMedia::Photo(url) => {
                let mut request = self
                    .bot
                    .send_photo(self.chat_id, InputFile::url(url.clone()));
                if let Some(caption) = &item.caption {
                    request = request.caption(caption.clone());
                }
                request.await.map_err(send)?;
            }
            ResolvedMedia::Video(url) => {
                let mut request = self
                    .bot
                    .send_video(self.chat_id, InputFile::url(url.clone()));
                if let Some(caption) = &item.caption {
                    request = request.caption(caption.clone());
                }
                request.await.map_err(send)?;
            }
            ResolvedMedia::Animation(url) => {
                let mut request = self
                    .bot
                    .send_animation(self.chat_id, InputFile::url(url.clone()));
                if let Some(caption) = &item.caption {
                    request = request.caption(caption.clone());
                }
                request.await.map_err(send)?;
            }
            ResolvedMedia::Link(url) => {
                // The caption already ends with the source URL; when it does
                // not contain the link we're embedding, append it.
                let text = match &item.caption {
                    Some(caption) if caption.contains(url.as_str()) => caption.clone(),
                    Some(caption) => format!("{caption}\n{url}"),
                    None => url.to_string(),
                };
                self.bot
                    .send_message(self.chat_id, text)
                    .await
                    .map_err(send)?;
            }
        }
        Ok(())
    }
}
