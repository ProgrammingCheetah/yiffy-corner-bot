use std::collections::HashSet;

use async_trait::async_trait;
use domain::elements::{
    post::PostId,
    report::{Report, ReportRepository, ReportRepositoryError},
};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct InMemoryReportRepository {
    /// (post id, reporter telegram id) — the dedupe key.
    reports: RwLock<HashSet<(u64, i64)>>,
}

impl InMemoryReportRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ReportRepository for InMemoryReportRepository {
    type Err = ReportRepositoryError;

    async fn add(&self, report: Report) -> Result<bool, Self::Err> {
        Ok(self
            .reports
            .write()
            .await
            .insert((*report.post_id.as_ref(), *report.reporter.as_ref())))
    }

    async fn count_for(&self, post_id: PostId) -> Result<u64, Self::Err> {
        Ok(self
            .reports
            .read()
            .await
            .iter()
            .filter(|(post, _)| post == post_id.as_ref())
            .count() as u64)
    }

    async fn clear_for(&self, post_id: PostId) -> Result<(), Self::Err> {
        self.reports
            .write()
            .await
            .retain(|(post, _)| post != post_id.as_ref());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use domain::elements::user::TelegramId;

    fn report(post: u64, reporter: i64) -> Report {
        Report {
            post_id: PostId::from(post),
            reporter: TelegramId::from(reporter),
            reported_at: Utc::now(),
            reason: Some("test reason".to_string()),
        }
    }

    #[tokio::test]
    async fn add_dedupes_per_reporter_and_counts() {
        let repo = InMemoryReportRepository::new();
        assert!(repo.add(report(1, 42)).await.unwrap());
        assert!(!repo.add(report(1, 42)).await.unwrap()); // duplicate
        assert!(repo.add(report(1, 43)).await.unwrap());
        assert_eq!(repo.count_for(PostId::from(1)).await.unwrap(), 2);

        repo.clear_for(PostId::from(1)).await.unwrap();
        assert_eq!(repo.count_for(PostId::from(1)).await.unwrap(), 0);
        // After dismissal a fresh report counts again.
        assert!(repo.add(report(1, 42)).await.unwrap());
    }
}
