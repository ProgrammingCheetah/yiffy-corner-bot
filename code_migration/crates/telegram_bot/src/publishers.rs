use async_trait::async_trait;
use domain::elements::publisher::{PublishItem, Publisher, PublisherError};
use teloxide::{Bot, types::ChatId};

/// Publishes resolved media to a Telegram channel.
///
/// Stub today: logs the intent and returns Ok. A later commit will turn this
/// into a real `send_photo` / `send_video` / `send_animation` / `send_message`
/// dispatch keyed on the [`ResolvedMedia`](domain::elements::media::ResolvedMedia) variant.
pub struct TelegramPublisher {
    pub bot: Bot,
    pub chat_id: ChatId,
}

impl TelegramPublisher {
    pub fn new(bot: Bot, chat_id: ChatId) -> Self {
        Self { bot, chat_id }
    }
}

#[async_trait]
impl Publisher for TelegramPublisher {
    async fn publish(&self, item: &PublishItem) -> Result<(), PublisherError> {
        tracing::info!(
            chat = self.chat_id.0,
            media = ?item.media,
            caption = ?item.caption,
            "stub publish — TelegramPublisher not yet wired to teloxide send"
        );
        Ok(())
    }
}
