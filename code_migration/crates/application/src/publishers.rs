use domain::elements::{post::Post, publisher::{PublishBlock, Publisher, PublisherError}};
use teloxide::prelude::*;




struct TelegramPublisher {
    pub bot: Bot,
    pub chat_id: ChatId,
    pub publish_block: PublishBlock
}
impl Publisher for TelegramPublisher {
    async fn publish(
        &self,
        post: &Post,
    ) -> Result<(), PublisherError> { 
        self.bot.send_message(, text)
    }

    async fn should_publish(&self, tick: PublishBlock) -> bool {
        tick % self.publish_block == 0
    }

}
