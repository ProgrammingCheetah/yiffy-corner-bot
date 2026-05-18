use async_trait::async_trait;
use domain::elements::{
    post::Post,
    publisher::{Publisher, PublisherError},
};
use teloxide::{Bot, types::ChatId};

/// Publishes Posts to a Telegram channel.
///
/// Stub today: logs the intent and returns Ok. A later commit will turn this
/// into a real `bot.send_photo` / `send_video` dispatch.
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
    async fn publish(&self, post: &Post) -> Result<(), PublisherError> {
        tracing::info!(
            chat = self.chat_id.0,
            post_id = %post.id,
            "stub publish — TelegramPublisher not yet wired to teloxide send"
        );
        Ok(())
    }
}
