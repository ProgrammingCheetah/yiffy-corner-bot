use std::collections::{BTreeMap, HashSet};

use async_trait::async_trait;
use domain::elements::{
    tag::Tag,
    tag_policy::{
        ForbiddenTagRepository, ForbiddenTagRepositoryError, RequiredTagRepository,
        RequiredTagRepositoryError, SpoilerTagRepository, SpoilerTagRepositoryError,
    },
};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct InMemoryForbiddenTagRepository {
    /// tag name → reason (BTreeMap for the tag-ordered listings).
    tags: RwLock<BTreeMap<String, Option<String>>>,
}

impl InMemoryForbiddenTagRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ForbiddenTagRepository for InMemoryForbiddenTagRepository {
    type Err = ForbiddenTagRepositoryError;

    async fn add(&self, tag: Tag, reason: Option<String>) -> Result<(), Self::Err> {
        self.tags
            .write()
            .await
            .insert(tag.as_ref().to_string(), reason);
        Ok(())
    }

    async fn remove(&self, tag: &Tag) -> Result<(), Self::Err> {
        self.tags.write().await.remove(tag.as_ref());
        Ok(())
    }

    async fn contains(&self, tag: &Tag) -> Result<bool, Self::Err> {
        Ok(self.tags.read().await.contains_key(tag.as_ref()))
    }

    async fn reason_for(&self, tag: &Tag) -> Result<Option<String>, Self::Err> {
        Ok(self.tags.read().await.get(tag.as_ref()).cloned().flatten())
    }

    async fn list_all(&self) -> Result<Vec<Tag>, Self::Err> {
        Ok(self
            .tags
            .read()
            .await
            .keys()
            .map(|name| Tag::from(name.as_str()))
            .collect())
    }

    async fn list_with_reasons(&self) -> Result<Vec<(Tag, Option<String>)>, Self::Err> {
        Ok(self
            .tags
            .read()
            .await
            .iter()
            .map(|(name, reason)| (Tag::from(name.as_str()), reason.clone()))
            .collect())
    }
}

#[derive(Debug, Default)]
pub struct InMemoryRequiredTagRepository {
    tags: RwLock<HashSet<Tag>>,
}

impl InMemoryRequiredTagRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl RequiredTagRepository for InMemoryRequiredTagRepository {
    type Err = RequiredTagRepositoryError;

    async fn add(&self, tag: Tag) -> Result<(), Self::Err> {
        self.tags.write().await.insert(tag);
        Ok(())
    }

    async fn remove(&self, tag: &Tag) -> Result<(), Self::Err> {
        self.tags.write().await.remove(tag);
        Ok(())
    }

    async fn contains(&self, tag: &Tag) -> Result<bool, Self::Err> {
        Ok(self.tags.read().await.contains(tag))
    }

    async fn list_all(&self) -> Result<Vec<Tag>, Self::Err> {
        Ok(self.tags.read().await.iter().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn forbidden_add_then_contains() {
        let repo = InMemoryForbiddenTagRepository::new();
        repo.add(Tag::from("fox"), None).await.unwrap();
        assert!(repo.contains(&Tag::from("fox")).await.unwrap());
        assert!(!repo.contains(&Tag::from("wolf")).await.unwrap());
    }

    #[tokio::test]
    async fn forbidden_remove_drops_membership() {
        let repo = InMemoryForbiddenTagRepository::new();
        repo.add(Tag::from("fox"), None).await.unwrap();
        repo.remove(&Tag::from("fox")).await.unwrap();
        assert!(!repo.contains(&Tag::from("fox")).await.unwrap());
    }

    #[tokio::test]
    async fn forbidden_add_is_idempotent() {
        let repo = InMemoryForbiddenTagRepository::new();
        repo.add(Tag::from("fox"), None).await.unwrap();
        repo.add(Tag::from("fox"), None).await.unwrap();
        assert_eq!(repo.list_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn forbidden_list_all_returns_every_added() {
        let repo = InMemoryForbiddenTagRepository::new();
        repo.add(Tag::from("a"), None).await.unwrap();
        repo.add(Tag::from("b"), None).await.unwrap();
        let mut all = repo.list_all().await.unwrap();
        all.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        assert_eq!(all, vec![Tag::from("a"), Tag::from("b")]);
    }

    #[tokio::test]
    async fn required_add_then_contains() {
        let repo = InMemoryRequiredTagRepository::new();
        repo.add(Tag::from("furry")).await.unwrap();
        assert!(repo.contains(&Tag::from("furry")).await.unwrap());
        assert!(!repo.contains(&Tag::from("anime")).await.unwrap());
    }

    #[tokio::test]
    async fn required_remove_drops_membership() {
        let repo = InMemoryRequiredTagRepository::new();
        repo.add(Tag::from("furry")).await.unwrap();
        repo.remove(&Tag::from("furry")).await.unwrap();
        assert!(!repo.contains(&Tag::from("furry")).await.unwrap());
    }
}

#[derive(Debug, Default)]
pub struct InMemorySpoilerTagRepository {
    tags: RwLock<HashSet<Tag>>,
}

impl InMemorySpoilerTagRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SpoilerTagRepository for InMemorySpoilerTagRepository {
    type Err = SpoilerTagRepositoryError;

    async fn add(&self, tag: Tag) -> Result<(), Self::Err> {
        self.tags.write().await.insert(tag);
        Ok(())
    }

    async fn remove(&self, tag: &Tag) -> Result<(), Self::Err> {
        self.tags.write().await.remove(tag);
        Ok(())
    }

    async fn contains(&self, tag: &Tag) -> Result<bool, Self::Err> {
        Ok(self.tags.read().await.contains(tag))
    }

    async fn list_all(&self) -> Result<Vec<Tag>, Self::Err> {
        Ok(self.tags.read().await.iter().cloned().collect())
    }
}
