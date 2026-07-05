//! SQLite adapters (sqlx) for every repository port.
//!
//! Queries are runtime-checked (no `DATABASE_URL` needed at build time); the
//! schema lives in `migrations/` and is embedded via [`sqlx::migrate!`].

pub mod post;
pub mod poster;
pub mod publisher_config;
pub mod tag_policy;
#[cfg(test)]
mod tests;
pub mod user;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

/// Open (creating the file if needed) and migrate a SQLite database.
pub async fn connect_and_migrate(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options: SqliteConnectOptions = database_url
        .parse::<SqliteConnectOptions>()?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new().connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
pub(crate) async fn test_pool() -> SqlitePool {
    // One connection so every query in a test sees the same :memory: DB.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    pool
}
