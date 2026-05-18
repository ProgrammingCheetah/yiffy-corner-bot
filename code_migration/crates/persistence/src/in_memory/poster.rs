use std::{
    collections::HashMap,
    sync::{RwLock, atomic::AtomicU64},
};

use domain::elements::poster::Poster;

#[derive(Debug, Default)]
pub struct InMemoryPosterRepository {
    posters: RwLock<HashMap<u64, Poster>>,
    next_id: AtomicU64,
}

impl InMemoryPosterRepository {
    pub fn new() -> Self {
        Self::default()
    }
}
