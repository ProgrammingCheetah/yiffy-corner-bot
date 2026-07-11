use async_trait::async_trait;
use domain::elements::{
    post::PostId,
    report::{Report, ReportRepository, ReportRepositoryError},
};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct InMemoryReportRepository {
    /// Full reports; (post id, reporter telegram id) stays the dedupe key.
    reports: RwLock<Vec<Report>>,
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
        let mut reports = self.reports.write().await;
        if reports
            .iter()
            .any(|r| r.post_id == report.post_id && r.reporter == report.reporter)
        {
            return Ok(false);
        }
        reports.push(report);
        Ok(true)
    }

    async fn count_for(&self, post_id: PostId) -> Result<u64, Self::Err> {
        Ok(self
            .reports
            .read()
            .await
            .iter()
            .filter(|r| r.post_id == post_id)
            .count() as u64)
    }

    async fn list_all(&self) -> Result<Vec<Report>, Self::Err> {
        let mut all = self.reports.read().await.clone();
        all.sort_by(|a, b| b.reported_at.cmp(&a.reported_at));
        Ok(all)
    }

    async fn clear_for(&self, post_id: PostId) -> Result<(), Self::Err> {
        self.reports.write().await.retain(|r| r.post_id != post_id);
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
            reporter_username: None,
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
