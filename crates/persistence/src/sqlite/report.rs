use async_trait::async_trait;
use domain::elements::{
    post::PostId,
    report::{Report, ReportRepository, ReportRepositoryError},
};
use sqlx::{Row, sqlite::SqlitePool};

#[derive(Clone)]
pub struct SqliteReportRepository {
    pool: SqlitePool,
}

impl SqliteReportRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReportRepository for SqliteReportRepository {
    type Err = ReportRepositoryError;

    async fn add(&self, report: Report) -> Result<bool, Self::Err> {
        let result = sqlx::query(
            "INSERT INTO reports (post_id, reporter_telegram_id, reported_at, reason)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (post_id, reporter_telegram_id) DO NOTHING",
        )
        .bind(*report.post_id.as_ref() as i64)
        .bind(*report.reporter.as_ref())
        .bind(report.reported_at)
        .bind(&report.reason)
        .execute(&self.pool)
        .await
        .map_err(|e| ReportRepositoryError::Storage(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_all(&self) -> Result<Vec<Report>, Self::Err> {
        let rows = sqlx::query(
            "SELECT post_id, reporter_telegram_id, reported_at, reason
             FROM reports ORDER BY reported_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ReportRepositoryError::Storage(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| Report {
                post_id: PostId::from(row.get::<i64, _>("post_id") as u64),
                reporter: domain::elements::user::TelegramId::from(
                    row.get::<i64, _>("reporter_telegram_id"),
                ),
                reported_at: row.get("reported_at"),
                reason: row.get("reason"),
            })
            .collect())
    }

    async fn count_for(&self, post_id: PostId) -> Result<u64, Self::Err> {
        let row = sqlx::query("SELECT COUNT(*) AS n FROM reports WHERE post_id = ?")
            .bind(*post_id.as_ref() as i64)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ReportRepositoryError::Storage(e.to_string()))?;
        Ok(row.get::<i64, _>("n") as u64)
    }

    async fn clear_for(&self, post_id: PostId) -> Result<(), Self::Err> {
        sqlx::query("DELETE FROM reports WHERE post_id = ?")
            .bind(*post_id.as_ref() as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| ReportRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::{test_pool, user::SqliteUserRepository};
    use chrono::Utc;
    use domain::elements::post::{PostRepository as _, PostStatus, Source};
    use domain::elements::user::TelegramId;
    use domain::elements::user::{Role, UserRepository as _};
    use url::Url;

    async fn seeded_post(pool: &SqlitePool) -> PostId {
        // FK: reports reference posts.
        let posts = crate::sqlite::post::SqlitePostRepository::new(pool.clone());
        posts
            .create(
                Source::try_from(Url::parse("https://e621.net/posts/1").unwrap()).unwrap(),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::Accepted,
            )
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn add_dedupes_counts_and_clears() {
        let pool = test_pool().await;
        // Ensure a registered user exists just to mirror real traffic (not
        // required by the schema — reporters are raw telegram ids).
        SqliteUserRepository::new(pool.clone())
            .create(TelegramId::from(42), Role::User, None, None)
            .await
            .unwrap();
        let post_id = seeded_post(&pool).await;
        let repo = SqliteReportRepository::new(pool);

        let report = |reporter: i64| Report {
            post_id,
            reporter: TelegramId::from(reporter),
            reported_at: Utc::now(),
            reason: Some("gore, not tagged".to_string()),
        };
        assert!(repo.add(report(42)).await.unwrap());
        assert!(!repo.add(report(42)).await.unwrap());
        assert!(repo.add(report(43)).await.unwrap());
        assert_eq!(repo.count_for(post_id).await.unwrap(), 2);

        repo.clear_for(post_id).await.unwrap();
        assert_eq!(repo.count_for(post_id).await.unwrap(), 0);
        assert!(repo.add(report(42)).await.unwrap());
    }
}
