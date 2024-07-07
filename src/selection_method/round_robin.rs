use std::{
    // hash::Hasher,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

pub struct RoundRobin(AtomicUsize);

impl SelectionAlgorithm for RoundRobin {
    fn new() -> Self {
        Self(AtomicUsize::new(0))
    }
    fn next(&self, _key: &[u8]) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed) as u64
    }
}

pub struct RoundRobinSelector {
    backends: Vec<Backend>,
    current_index: usize,
}
impl BackendSelection for RoundRobinSelector {
    fn build(backends: Vec<Backend>) -> Self {
        Self {
            backends,
            current_index: 0,
        }
    }

    fn get_next(&mut self) -> SocketAddr {
        let addr: SocketAddr = self.backends[self.current_index].addr;
        self.current_index = (self.current_index + 1) % self.backends.len();
        addr
    }
}
