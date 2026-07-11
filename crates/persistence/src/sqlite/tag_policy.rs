use async_trait::async_trait;
use domain::elements::{
    tag::Tag,
    tag_policy::{
        ForbiddenTagRepository, ForbiddenTagRepositoryError, RequiredTagRepository,
        RequiredTagRepositoryError, SpoilerTagRepository, SpoilerTagRepositoryError,
    },
};
use sqlx::{Row, sqlite::SqlitePool};

macro_rules! sqlite_tag_repository {
    ($name:ident, $port:ident, $error:ident, $table:literal) => {
        #[derive(Clone)]
        pub struct $name {
            pool: SqlitePool,
        }

        impl $name {
            pub fn new(pool: SqlitePool) -> Self {
                Self { pool }
            }
        }

        #[async_trait]
        impl $port for $name {
            type Err = $error;

            async fn add(&self, tag: Tag) -> Result<(), Self::Err> {
                sqlx::query(concat!(
                    "INSERT INTO ",
                    $table,
                    " (tag) VALUES (?) ON CONFLICT (tag) DO NOTHING"
                ))
                .bind(tag.as_ref())
                .execute(&self.pool)
                .await
                .map_err(|e| $error::Storage(e.to_string()))?;
                Ok(())
            }

            async fn remove(&self, tag: &Tag) -> Result<(), Self::Err> {
                sqlx::query(concat!("DELETE FROM ", $table, " WHERE tag = ?"))
                    .bind(tag.as_ref())
                    .execute(&self.pool)
                    .await
                    .map_err(|e| $error::Storage(e.to_string()))?;
                Ok(())
            }

            async fn contains(&self, tag: &Tag) -> Result<bool, Self::Err> {
                let row = sqlx::query(concat!("SELECT 1 FROM ", $table, " WHERE tag = ? LIMIT 1"))
                    .bind(tag.as_ref())
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| $error::Storage(e.to_string()))?;
                Ok(row.is_some())
            }

            async fn list_all(&self) -> Result<Vec<Tag>, Self::Err> {
                let rows = sqlx::query(concat!("SELECT tag FROM ", $table, " ORDER BY tag"))
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|e| $error::Storage(e.to_string()))?;
                Ok(rows
                    .iter()
                    .map(|row| Tag::from(row.get::<String, _>("tag")))
                    .collect())
            }
        }
    };
}

// Forbidden tags carry a reason, so their repository has outgrown the
// macro the reason-less registries share.
#[derive(Clone)]
pub struct SqliteForbiddenTagRepository {
    pool: SqlitePool,
}

impl SqliteForbiddenTagRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ForbiddenTagRepository for SqliteForbiddenTagRepository {
    type Err = ForbiddenTagRepositoryError;

    async fn add(&self, tag: Tag, reason: Option<String>) -> Result<(), Self::Err> {
        sqlx::query(
            "INSERT INTO forbidden_tags (tag, reason) VALUES (?, ?)
             ON CONFLICT (tag) DO UPDATE SET reason = excluded.reason",
        )
        .bind(tag.as_ref())
        .bind(reason)
        .execute(&self.pool)
        .await
        .map_err(|e| ForbiddenTagRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn remove(&self, tag: &Tag) -> Result<(), Self::Err> {
        sqlx::query("DELETE FROM forbidden_tags WHERE tag = ?")
            .bind(tag.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| ForbiddenTagRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn contains(&self, tag: &Tag) -> Result<bool, Self::Err> {
        let row = sqlx::query("SELECT 1 FROM forbidden_tags WHERE tag = ? LIMIT 1")
            .bind(tag.as_ref())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| ForbiddenTagRepositoryError::Storage(e.to_string()))?;
        Ok(row.is_some())
    }

    async fn reason_for(&self, tag: &Tag) -> Result<Option<String>, Self::Err> {
        let row = sqlx::query("SELECT reason FROM forbidden_tags WHERE tag = ?")
            .bind(tag.as_ref())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| ForbiddenTagRepositoryError::Storage(e.to_string()))?;
        Ok(row.and_then(|r| r.get::<Option<String>, _>("reason")))
    }

    async fn list_all(&self) -> Result<Vec<Tag>, Self::Err> {
        let rows = sqlx::query("SELECT tag FROM forbidden_tags ORDER BY tag")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ForbiddenTagRepositoryError::Storage(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| Tag::from(row.get::<String, _>("tag")))
            .collect())
    }

    async fn list_with_reasons(&self) -> Result<Vec<(Tag, Option<String>)>, Self::Err> {
        let rows = sqlx::query("SELECT tag, reason FROM forbidden_tags ORDER BY tag")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ForbiddenTagRepositoryError::Storage(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| {
                (
                    Tag::from(row.get::<String, _>("tag")),
                    row.get::<Option<String>, _>("reason"),
                )
            })
            .collect())
    }
}

sqlite_tag_repository!(
    SqliteRequiredTagRepository,
    RequiredTagRepository,
    RequiredTagRepositoryError,
    "required_tags"
);
sqlite_tag_repository!(
    SqliteSpoilerTagRepository,
    SpoilerTagRepository,
    SpoilerTagRepositoryError,
    "spoiler_tags"
);
