use async_trait::async_trait;
use domain::elements::{
    cadence::PostInterval,
    poster::{Poster, PosterId, PosterRepository, PosterRepositoryError},
    tag::Tag,
};
use sqlx::{Row, sqlite::SqlitePool};

#[derive(Clone)]
pub struct SqlitePosterRepository {
    pool: SqlitePool,
}

impl SqlitePosterRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Tags are space-joined in storage (e621 tags cannot contain spaces).
fn join_tags(tags: &[Tag]) -> String {
    tags.iter()
        .map(|t| t.as_ref())
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_tags(joined: &str) -> Vec<Tag> {
    joined.split_whitespace().map(Tag::from).collect()
}

fn row_to_poster(row: &sqlx::sqlite::SqliteRow) -> Result<Poster, PosterRepositoryError> {
    let interval: i64 = row.get("time_interval");
    Ok(Poster {
        id: PosterId::from(row.get::<i64, _>("id") as u64),
        subscribed_tags: split_tags(&row.get::<String, _>("subscribed_tags")),
        forbidden_tags: split_tags(&row.get::<String, _>("forbidden_tags")),
        time_interval: PostInterval::new(interval as u8)
            .map_err(|e| PosterRepositoryError::NotCreated(e.to_string()))?,
        cursor: row.get::<i64, _>("cursor") as u64,
    })
}

#[async_trait]
impl PosterRepository for SqlitePosterRepository {
    type Err = PosterRepositoryError;

    async fn create(
        &self,
        subscribed_tags: Vec<Tag>,
        forbidden_tags: Vec<Tag>,
        time_interval: PostInterval,
    ) -> Result<Poster, Self::Err> {
        let row = sqlx::query(
            "INSERT INTO posters (subscribed_tags, forbidden_tags, time_interval)
             VALUES (?, ?, ?) RETURNING *",
        )
        .bind(join_tags(&subscribed_tags))
        .bind(join_tags(&forbidden_tags))
        .bind(*time_interval.as_ref() as i64)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| PosterRepositoryError::NotCreated(e.to_string()))?;
        row_to_poster(&row)
    }

    async fn find_by_id(&self, id: PosterId) -> Result<Option<Poster>, Self::Err> {
        let row = sqlx::query("SELECT * FROM posters WHERE id = ?")
            .bind(*id.as_ref() as i64)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| PosterRepositoryError::NotCreated(e.to_string()))?;
        row.as_ref().map(row_to_poster).transpose()
    }

    async fn set_tags(
        &self,
        id: PosterId,
        subscribed_tags: Vec<Tag>,
        forbidden_tags: Vec<Tag>,
    ) -> Result<Poster, Self::Err> {
        let row = sqlx::query(
            "UPDATE posters SET subscribed_tags = ?, forbidden_tags = ? WHERE id = ? RETURNING *",
        )
        .bind(join_tags(&subscribed_tags))
        .bind(join_tags(&forbidden_tags))
        .bind(*id.as_ref() as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PosterRepositoryError::NotCreated(e.to_string()))?
        .ok_or(PosterRepositoryError::NotFound(id))?;
        row_to_poster(&row)
    }

    async fn delete(&self, id: PosterId) -> Result<(), Self::Err> {
        let result = sqlx::query("DELETE FROM posters WHERE id = ?")
            .bind(*id.as_ref() as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| PosterRepositoryError::NotCreated(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(PosterRepositoryError::NotFound(id));
        }
        Ok(())
    }

    async fn set_cursor(&self, id: PosterId, cursor: u64) -> Result<(), Self::Err> {
        let result = sqlx::query("UPDATE posters SET cursor = ? WHERE id = ?")
            .bind(cursor as i64)
            .bind(*id.as_ref() as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| PosterRepositoryError::NotCreated(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(PosterRepositoryError::NotFound(id));
        }
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<Poster>, Self::Err> {
        let rows = sqlx::query("SELECT * FROM posters ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| PosterRepositoryError::NotCreated(e.to_string()))?;
        rows.iter().map(row_to_poster).collect()
    }
}
