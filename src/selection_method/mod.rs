use std::net::SocketAddr;

use crate::common::types::Backend;

pub mod round_robin;

pub trait SelectionAlgorithm {
    fn new() -> Self;
    fn next(&self, key: &[u8]) -> u64;
}

pub trait BackendSelection {
    fn build(backends: Vec<Backend>) -> Self;
    fn get_next(&mut self) -> SocketAddr;
}
pub trait BackendIter {
    /// Return `Some(&Backend)` when there are more backends left to choose from.
    fn next(&mut self) -> Option<&Backend>;
}
