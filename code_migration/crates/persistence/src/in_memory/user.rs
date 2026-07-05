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
        display_name: Option<String>,
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
            display_name,
            is_banned: false,
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

    async fn set_display_name(
        &self,
        id: UserId,
        display_name: Option<String>,
    ) -> Result<(), UserRepositoryError> {
        let mut users = self.users.write().await;
        let user = users
            .get_mut(id.as_ref())
            .ok_or_else(|| UserRepositoryError::NotChanged("user not found".into()))?;
        user.display_name = display_name;
        Ok(())
    }

    async fn set_banned(&self, id: UserId, banned: bool) -> Result<(), UserRepositoryError> {
        let mut users = self.users.write().await;
        let user = users
            .get_mut(id.as_ref())
            .ok_or_else(|| UserRepositoryError::NotChanged("user not found".into()))?;
        user.is_banned = banned;
        Ok(())
    }

    async fn list_by_role(&self, role: Role) -> Result<Vec<User>, UserRepositoryError> {
        Ok(self
            .users
            .read()
            .await
            .values()
            .filter(|u| u.role == role)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create(repo: &InMemoryUserRepository, telegram_id: i64, role: Role) -> User {
        repo.create(TelegramId::from(telegram_id), role, None, None)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn create_then_find_by_id_roundtrip() {
        let repo = InMemoryUserRepository::new();
        let user = create(&repo, 1402476143, Role::Owner).await;
        let found = repo.find_by_id(user.id).await.unwrap();
        assert_eq!(found.map(|u| u.id), Some(user.id));
    }

    #[tokio::test]
    async fn find_by_telegram_id_returns_user() {
        let repo = InMemoryUserRepository::new();
        let created = create(&repo, 42, Role::User).await;
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
        let a = create(&repo, 1, Role::User).await;
        let b = create(&repo, 2, Role::User).await;
        assert_ne!(a.id, b.id);
    }

    #[tokio::test]
    async fn create_stores_display_name_and_starts_unbanned() {
        let repo = InMemoryUserRepository::new();
        let user = repo
            .create(
                TelegramId::from(9),
                Role::User,
                None,
                Some("Ziel".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(user.display_name.as_deref(), Some("Ziel"));
        assert!(!user.is_banned);
    }

    #[tokio::test]
    async fn set_display_name_updates_user() {
        let repo = InMemoryUserRepository::new();
        let user = create(&repo, 9, Role::User).await;
        repo.set_display_name(user.id, Some("NewName".to_string()))
            .await
            .unwrap();
        let found = repo.find_by_id(user.id).await.unwrap().unwrap();
        assert_eq!(found.display_name.as_deref(), Some("NewName"));
    }

    #[tokio::test]
    async fn set_banned_flips_flag_both_ways() {
        let repo = InMemoryUserRepository::new();
        let user = create(&repo, 9, Role::User).await;
        repo.set_banned(user.id, true).await.unwrap();
        assert!(repo.find_by_id(user.id).await.unwrap().unwrap().is_banned);
        repo.set_banned(user.id, false).await.unwrap();
        assert!(!repo.find_by_id(user.id).await.unwrap().unwrap().is_banned);
    }

    #[tokio::test]
    async fn set_banned_unknown_user_errors() {
        let repo = InMemoryUserRepository::new();
        let err = repo.set_banned(UserId::from(42), true).await.unwrap_err();
        assert!(matches!(err, UserRepositoryError::NotChanged(_)));
    }

    #[tokio::test]
    async fn list_by_role_filters_correctly() {
        let repo = InMemoryUserRepository::new();
        create(&repo, 1, Role::Owner).await;
        create(&repo, 2, Role::Moderator).await;
        create(&repo, 3, Role::Moderator).await;
        create(&repo, 4, Role::User).await;
        assert_eq!(repo.list_by_role(Role::Owner).await.unwrap().len(), 1);
        assert_eq!(repo.list_by_role(Role::Moderator).await.unwrap().len(), 2);
        assert_eq!(repo.list_by_role(Role::User).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn create_rejects_duplicate_telegram_id() {
        let repo = InMemoryUserRepository::new();
        create(&repo, 7, Role::User).await;
        let err = repo
            .create(TelegramId::from(7), Role::User, None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, UserRepositoryError::NotCreated(_)));
    }
}
