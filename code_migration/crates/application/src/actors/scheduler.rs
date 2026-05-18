use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use domain::elements::{
    cadence::{PublishBlock, PublishBlockError},
    post::{PostRepository, PostSelectorStrategy},
    poster::Poster,
    publisher::Publisher,
};
use tokio::time::{MissedTickBehavior, interval};

/// One unit of scheduler work: a [`Poster`] (config) paired with the per-Poster
/// [`PostSelectorStrategy`] that picks its next [`Post`] and the [`Publisher`]
/// that delivers it.
pub struct PosterRuntime {
    pub poster: Poster,
    pub selector: Box<dyn PostSelectorStrategy>,
    pub publisher: Box<dyn Publisher>,
}

pub struct SchedulerDeps<R: PostRepository + Send + Sync> {
    pub runtimes: Vec<PosterRuntime>,
    pub posts: Arc<R>,
}

#[derive(Debug, thiserror::Error)]
pub enum TickError {
    #[error("invalid PublishBlock: {0}")]
    Block(#[from] PublishBlockError),
}

/// Run one scheduler tick: for every Poster whose interval divides the current
/// minute-of-hour, select a post, publish it, and record `last_posted`.
///
/// Per-runtime failures are logged and the tick continues with the next
/// runtime — one Poster failing must not block the others.
pub async fn run_tick<R>(
    now: DateTime<Utc>,
    runtimes: &[PosterRuntime],
    posts: &R,
) -> Result<(), TickError>
where
    R: PostRepository + Send + Sync,
    R::Err: std::fmt::Display,
{
    let block = PublishBlock::try_from(now.minute_of_hour())?;
    for rt in runtimes {
        if !block.fires_for(&rt.poster.time_interval) {
            continue;
        }
        let post = match rt.selector.find_post() {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(poster_id = %rt.poster.id, error = %e, "selector failed");
                continue;
            }
        };
        if let Err(e) = rt.publisher.publish(&post).await {
            tracing::error!(poster_id = %rt.poster.id, error = %e, "publish failed");
            continue;
        }
        if let Err(e) = posts.mark_posted(post.id, now).await {
            tracing::error!(poster_id = %rt.poster.id, error = %e, "mark_posted failed");
            continue;
        }
    }
    Ok(())
}

/// Loop forever, waking every minute to call [`run_tick`].
pub async fn start_scheduler<R>(deps: SchedulerDeps<R>) -> !
where
    R: PostRepository + Send + Sync + 'static,
    R::Err: std::fmt::Display,
{
    let mut ticker = interval(Duration::from_secs(60));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        let now = Utc::now();
        if let Err(e) = run_tick(now, &deps.runtimes, &*deps.posts).await {
            tracing::error!(error = %e, "scheduler tick failed");
        }
    }
}

/// Trait extension so `now.minute_of_hour()` reads as intent in `run_tick`.
trait MinuteOfHour {
    fn minute_of_hour(&self) -> u8;
}
impl MinuteOfHour for DateTime<Utc> {
    fn minute_of_hour(&self) -> u8 {
        use chrono::Timelike;
        self.minute() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use chrono::TimeZone;
    use domain::elements::{
        cadence::PostInterval,
        post::{
            ImgMimeSubtype, MimeType, PerceptualHash, Post, PostId, PostRepositoryError,
            PostStatus, SelectorError, Source,
        },
        poster::PosterId,
        publisher::PublisherError,
    };
    use url::Url;

    fn make_post(id: u64) -> Post {
        Post {
            id: PostId::from(id),
            media_type: MimeType::Image(ImgMimeSubtype::Png),
            sources: vec![Source::from(Url::parse("https://e621.net/p/1").unwrap())],
            tags: vec![],
            status: PostStatus::Accepted,
            last_posted: None,
            p_hash: PerceptualHash::from(0),
        }
    }

    fn make_poster(id: u64, interval_minutes: u8) -> Poster {
        Poster {
            id: PosterId::from(id),
            subscribed_tags: vec![],
            forbidden_tags: vec![],
            time_interval: PostInterval::new(interval_minutes).unwrap(),
        }
    }

    /// Selector that always returns a fixed Post.
    struct FixedSelector(Post);
    impl PostSelectorStrategy for FixedSelector {
        fn find_due_post(&self) -> Result<Option<Post>, SelectorError> {
            Ok(Some(self.0.clone()))
        }
        fn find_post(&self) -> Result<Post, SelectorError> {
            Ok(self.0.clone())
        }
    }

    /// Selector that always errors.
    struct ErroringSelector;
    impl PostSelectorStrategy for ErroringSelector {
        fn find_due_post(&self) -> Result<Option<Post>, SelectorError> {
            Err(SelectorError::NoMatch)
        }
        fn find_post(&self) -> Result<Post, SelectorError> {
            Err(SelectorError::NoMatch)
        }
    }

    /// Publisher that counts how many times `publish` was called.
    #[derive(Default)]
    struct CountingPublisher(Arc<AtomicUsize>);
    #[async_trait]
    impl Publisher for CountingPublisher {
        async fn publish(&self, _post: &Post) -> Result<(), PublisherError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// PostRepository that only records `mark_posted` calls.
    #[derive(Default)]
    struct RecordingPostRepository {
        marked: Mutex<Vec<(PostId, DateTime<Utc>)>>,
    }
    #[async_trait]
    impl PostRepository for RecordingPostRepository {
        type Err = PostRepositoryError;
        async fn create(
            &self,
            _media_type: MimeType,
            _sources: Vec<Source>,
            _tags: Vec<domain::elements::tag::Tag>,
            _p_hash: PerceptualHash,
        ) -> Result<Post, Self::Err> {
            unimplemented!("not needed by scheduler tests")
        }
        async fn find_by_id(&self, _id: PostId) -> Result<Option<Post>, Self::Err> {
            unimplemented!("not needed by scheduler tests")
        }
        async fn remove(&self, _id: PostId) -> Result<(), Self::Err> {
            unimplemented!("not needed by scheduler tests")
        }
        async fn set_status_to(
            &self,
            _id: PostId,
            _status: PostStatus,
        ) -> Result<(), Self::Err> {
            unimplemented!("not needed by scheduler tests")
        }
        async fn mark_posted(&self, id: PostId, at: DateTime<Utc>) -> Result<(), Self::Err> {
            self.marked.lock().unwrap().push((id, at));
            Ok(())
        }
    }

    fn at_minute(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 17, 14, minute, 0).unwrap()
    }

    #[tokio::test]
    async fn fires_when_block_matches_interval() {
        let count = Arc::new(AtomicUsize::new(0));
        let runtimes = vec![PosterRuntime {
            poster: make_poster(1, 5),
            selector: Box::new(FixedSelector(make_post(100))),
            publisher: Box::new(CountingPublisher(count.clone())),
        }];
        let posts = RecordingPostRepository::default();
        let now = at_minute(5); // 5 % 5 == 0 → fires

        run_tick(now, &runtimes, &posts).await.unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 1);
        let marked = posts.marked.lock().unwrap();
        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].0, PostId::from(100));
        assert_eq!(marked[0].1, now);
    }

    #[tokio::test]
    async fn does_not_fire_when_block_does_not_match() {
        let count = Arc::new(AtomicUsize::new(0));
        let runtimes = vec![PosterRuntime {
            poster: make_poster(1, 5),
            selector: Box::new(FixedSelector(make_post(100))),
            publisher: Box::new(CountingPublisher(count.clone())),
        }];
        let posts = RecordingPostRepository::default();
        let now = at_minute(7); // 7 % 5 != 0 → no fire

        run_tick(now, &runtimes, &posts).await.unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(posts.marked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn selector_error_does_not_abort_other_runtimes() {
        let count = Arc::new(AtomicUsize::new(0));
        let runtimes = vec![
            PosterRuntime {
                poster: make_poster(1, 5),
                selector: Box::new(ErroringSelector),
                publisher: Box::new(CountingPublisher(count.clone())),
            },
            PosterRuntime {
                poster: make_poster(2, 5),
                selector: Box::new(FixedSelector(make_post(200))),
                publisher: Box::new(CountingPublisher(count.clone())),
            },
        ];
        let posts = RecordingPostRepository::default();
        let now = at_minute(5);

        run_tick(now, &runtimes, &posts).await.unwrap();

        // First runtime's selector failed → no publish.
        // Second runtime fires normally → one publish.
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(posts.marked.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn two_runtimes_with_different_intervals_both_fire_at_block_zero() {
        let count_a = Arc::new(AtomicUsize::new(0));
        let count_b = Arc::new(AtomicUsize::new(0));
        let runtimes = vec![
            PosterRuntime {
                poster: make_poster(1, 5),
                selector: Box::new(FixedSelector(make_post(100))),
                publisher: Box::new(CountingPublisher(count_a.clone())),
            },
            PosterRuntime {
                poster: make_poster(2, 15),
                selector: Box::new(FixedSelector(make_post(200))),
                publisher: Box::new(CountingPublisher(count_b.clone())),
            },
        ];
        let posts = RecordingPostRepository::default();
        let now = at_minute(0); // divisible by every valid interval

        run_tick(now, &runtimes, &posts).await.unwrap();

        assert_eq!(count_a.load(Ordering::SeqCst), 1);
        assert_eq!(count_b.load(Ordering::SeqCst), 1);
        assert_eq!(posts.marked.lock().unwrap().len(), 2);
    }
}
