use std::str::FromStr;

use async_trait::async_trait;
use domain::elements::user::{Role, TelegramId, User, UserId, UserRepository, UserRepositoryError};
use sqlx::{Row, sqlite::SqlitePool};

#[derive(Clone)]
pub struct SqliteUserRepository {
    pool: SqlitePool,
}

impl SqliteUserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn row_to_user(row: &sqlx::sqlite::SqliteRow) -> Result<User, UserRepositoryError> {
    let role: String = row
        .try_get("role")
        .map_err(|e| UserRepositoryError::NotCreated(e.to_string()))?;
    Ok(User {
        id: UserId::from(row.get::<i64, _>("id") as u64),
        telegram_id: TelegramId::from(row.get::<i64, _>("telegram_id")),
        role: Role::from_str(&role).map_err(|e| UserRepositoryError::NotCreated(e.to_string()))?,
        added_by: row
            .get::<Option<i64>, _>("added_by")
            .map(|id| UserId::from(id as u64)),
        display_name: row.get("display_name"),
        is_banned: row.get::<i64, _>("is_banned") != 0,
    })
}

#[async_trait]
impl UserRepository for SqliteUserRepository {
    async fn create(
        &self,
        telegram_id: TelegramId,
        role: Role,
        added_by: Option<UserId>,
        display_name: Option<String>,
    ) -> Result<User, UserRepositoryError> {
        let result = sqlx::query(
            "INSERT INTO users (telegram_id, role, added_by, display_name, is_banned)
             VALUES (?, ?, ?, ?, 0) RETURNING *",
        )
        .bind(*telegram_id.as_ref())
        .bind(role.to_string())
        .bind(added_by.map(|id| *id.as_ref() as i64))
        .bind(&display_name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| UserRepositoryError::NotCreated(e.to_string()))?;
        row_to_user(&result)
    }

    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, UserRepositoryError> {
        let row = sqlx::query("SELECT * FROM users WHERE id = ?")
            .bind(*id.as_ref() as i64)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::NotChanged(e.to_string()))?;
        row.as_ref().map(row_to_user).transpose()
    }

    async fn find_by_telegram_id(
        &self,
        telegram_id: TelegramId,
    ) -> Result<Option<User>, UserRepositoryError> {
        let row = sqlx::query("SELECT * FROM users WHERE telegram_id = ?")
            .bind(*telegram_id.as_ref())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::NotChanged(e.to_string()))?;
        row.as_ref().map(row_to_user).transpose()
    }

    async fn change_role(&self, id: UserId, new_role: Role) -> Result<User, UserRepositoryError> {
        let row = sqlx::query("UPDATE users SET role = ? WHERE id = ? RETURNING *")
            .bind(new_role.to_string())
            .bind(*id.as_ref() as i64)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::NotChanged(e.to_string()))?
            .ok_or_else(|| UserRepositoryError::NotChanged("user not found".into()))?;
        row_to_user(&row)
    }

    async fn set_display_name(
        &self,
        id: UserId,
        display_name: Option<String>,
    ) -> Result<(), UserRepositoryError> {
        let result = sqlx::query("UPDATE users SET display_name = ? WHERE id = ?")
            .bind(&display_name)
            .bind(*id.as_ref() as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::NotChanged(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(UserRepositoryError::NotChanged("user not found".into()));
        }
        Ok(())
    }

    async fn set_banned(&self, id: UserId, banned: bool) -> Result<(), UserRepositoryError> {
        let result = sqlx::query("UPDATE users SET is_banned = ? WHERE id = ?")
            .bind(banned as i64)
            .bind(*id.as_ref() as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::NotChanged(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(UserRepositoryError::NotChanged("user not found".into()));
        }
        Ok(())
    }

    async fn list_by_role(&self, role: Role) -> Result<Vec<User>, UserRepositoryError> {
        let rows = sqlx::query("SELECT * FROM users WHERE role = ? ORDER BY id")
            .bind(role.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::NotChanged(e.to_string()))?;
        rows.iter().map(row_to_user).collect()
    }
}
