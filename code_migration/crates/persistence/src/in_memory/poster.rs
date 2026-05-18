use std::{
    collections::HashMap,
    sync::{RwLock, atomic::AtomicU64},
};

use domain::elements::{
    post::PostSelectorStrategy,
    poster::{Poster, PosterRepository},
};

#[derive(Debug, Default)]
pub struct InMemoryPosterRepository {
    posters: RwLock<HashMap<u64, Poster>>,
    next_id: AtomicU64,
}

impl PostSelectorStrategy for InMemoryPosterRepository {
    fn find_due_post(&self) -> Result<Option<domain::elements::post::Post>, Self::Err> {
        todo!()
    }

    fn find_post(&self) -> Result<domain::elements::post::Post, Self::Err> {
        todo!()
    }
}
