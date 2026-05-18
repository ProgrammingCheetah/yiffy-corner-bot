use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use domain::elements::user::{
    Role, TelegramId, User, UserId, UserRepository, UserRepositoryError,
};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryUserRepository {
    users: RwLock<HashMap<u64, User>>,
    next_id: AtomicU64,
}

impl InMemoryUserRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl UserRepository for InMemoryUserRepository {
    async fn create(
        &self,
        telegram_id: TelegramId,
        role: Role,
        added_by: Option<UserId>,
    ) -> Result<User, UserRepositoryError> {
        let mut users = self.users.write().await;
        if users.values().any(|u| u.telegram_id == telegram_id) {
            return Err(UserRepositoryError::NotCreated(format!(
                "telegram id {} already registered",
                telegram_id.as_ref()
            )));
        }
        let raw_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let user = User {
            id: UserId::from(raw_id),
            telegram_id,
            role,
            added_by,
        };
        users.insert(raw_id, user.clone());
        Ok(user)
    }

    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, UserRepositoryError> {
        Ok(self.users.read().await.get(id.as_ref()).cloned())
    }

    async fn find_by_telegram_id(
        &self,
        telegram_id: TelegramId,
    ) -> Result<Option<User>, UserRepositoryError> {
        Ok(self
            .users
            .read()
            .await
            .values()
            .find(|u| u.telegram_id == telegram_id)
            .cloned())
    }

    async fn change_role(&self, id: UserId, new_role: Role) -> Result<User, UserRepositoryError> {
        let mut users = self.users.write().await;
        let user = users
            .get_mut(id.as_ref())
            .ok_or_else(|| UserRepositoryError::NotChanged("user not found".into()))?;
        user.role = new_role;
        Ok(user.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_then_find_by_id_roundtrip() {
        let repo = InMemoryUserRepository::new();
        let user = repo
            .create(TelegramId::from(1402476143), Role::Owner, None)
            .await
            .unwrap();
        let found = repo.find_by_id(user.id).await.unwrap();
        assert_eq!(found.map(|u| u.id), Some(user.id));
    }

    #[tokio::test]
    async fn find_by_telegram_id_returns_user() {
        let repo = InMemoryUserRepository::new();
        let created = repo
            .create(TelegramId::from(42), Role::User, None)
            .await
            .unwrap();
        let found = repo
            .find_by_telegram_id(TelegramId::from(42))
            .await
            .unwrap();
        assert_eq!(found.map(|u| u.id), Some(created.id));
    }

    #[tokio::test]
    async fn find_unknown_returns_none() {
        let repo = InMemoryUserRepository::new();
        assert!(repo.find_by_id(UserId::from(42)).await.unwrap().is_none());
        assert!(
            repo.find_by_telegram_id(TelegramId::from(42))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn create_assigns_unique_ids() {
        let repo = InMemoryUserRepository::new();
        let a = repo
            .create(TelegramId::from(1), Role::User, None)
            .await
            .unwrap();
        let b = repo
            .create(TelegramId::from(2), Role::User, None)
            .await
            .unwrap();
        assert_ne!(a.id, b.id);
    }

    #[tokio::test]
    async fn create_rejects_duplicate_telegram_id() {
        let repo = InMemoryUserRepository::new();
        repo.create(TelegramId::from(7), Role::User, None)
            .await
            .unwrap();
        let err = repo
            .create(TelegramId::from(7), Role::User, None)
            .await
            .unwrap_err();
        assert!(matches!(err, UserRepositoryError::NotCreated(_)));
    }
}
