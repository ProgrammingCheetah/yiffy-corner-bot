use crate::elements::post::Post;

#[derive(Debug, PartialEq, PartialOrd, Eq)]
pub struct PublishBlock(u8);

impl PublishBlock {
    pub fn new(value: u8) -> Result<Self, PublishBlockError> {
        let value = value.clamp(1, 60);
        Ok(Self(value))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PublishBlockError {
    #[error("Error Parsing ")]
    ParseError,
    #[error("Invalid value")]
    OutOfRangeError,
}

impl TryFrom<u8> for PublishBlock {
    type Error = PublishBlockError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PublisherError {}

#[async_trait::async_trait]
pub trait Publisher: Send + Sync {
    async fn publish(&self, post: &Post) -> Result<(), PublisherError>;
    async fn should_publish(&self, tick: PublishBlock) -> bool;
}
