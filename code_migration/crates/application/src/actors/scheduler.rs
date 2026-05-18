use std::{sync::Arc, time::Duration};

use domain::elements::post::PostSelectorStrategy;
use tokio::time::interval;

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("Could not start scheduler: {0}")]
    NotStarted(String),
}

pub struct Deps {
    post_service: Box<dyn PostSelectorStrategy>,
}

async fn start_scheduler(deps: Arc<Deps>) {
    let mut ticker = interval(Duration::from_mins(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        if let Err(e) = run_tick(&*deps.post_service).await {}
    }
}

#[derive(Debug, thiserror::Error)]
enum TickError {}
async fn run_tick<T>(post_service: &T) -> Result<(), TickError>
where
    T: PostSelectorStrategy,
{
}
