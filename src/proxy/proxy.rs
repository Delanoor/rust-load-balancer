use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use rand::Rng;
use tokio::sync::RwLock;

use crate::configuration::LoadBalancingAlgorithm;

use crate::proxy::backend::Backend;

#[derive(Debug)]
pub struct Proxy {
    pub backends: Vec<Arc<Backend>>,
    pub selector: LoadBalancingAlgorithm,
    pub current_index: Mutex<usize>,
    pub connection_counts: Arc<RwLock<HashMap<SocketAddr, usize>>>,
}

impl Proxy {
    pub async fn get_next(&self) -> SocketAddr {
        match self.selector {
            LoadBalancingAlgorithm::Random => {
                let mut rng = rand::thread_rng();
                let random_index = rng.gen_range(0..self.backends.len());
                // info!("Selected random backend index: {}", random_index);
                self.backends[random_index].listen_addr
            }
            LoadBalancingAlgorithm::RoundRobin => {
                let mut index = self.current_index.lock().unwrap();
                let addr = self.backends[*index].listen_addr;
                *index = (*index + 1) % self.backends.len();
                // info!("Selected round-robin backend index: {}", *index);
                addr
            }
            LoadBalancingAlgorithm::LeastConnection => self.select_least_connection_backend().await,
        }
    }

    async fn select_least_connection_backend(&self) -> SocketAddr {
        let counts = self.connection_counts.read().await;
        tracing::info!("counts: {:?} ", counts);
        let addr = self
            .backends
            .iter()
            .map(|backend| backend.listen_addr)
            .min_by_key(|&addr| counts.get(&addr).cloned().unwrap_or(usize::MAX))
            .unwrap();

        tracing::info!("Selected least-connection backend: {}", addr);
        addr
    }

    pub async fn update_connection_count(&self, addr: SocketAddr, delta: isize) {
        let mut counts = self.connection_counts.write().await;
        let count = counts.entry(addr).or_insert(0);
        *count = (*count as isize + delta) as usize
    }
}
