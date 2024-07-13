use rand::Rng;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

pub mod backend;
use crate::configuration::{LoadBalancingAlgorithm, Settings};
use backend::Backend;

#[derive(Debug)]
pub struct LoadBalancer {
    pub listen_addr: SocketAddr,
    pub proxies: Arc<RwLock<Proxy>>,
    rx: Receiver<Settings>,
}

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
        info!("counts: {:?} ", counts);
        let addr = self
            .backends
            .iter()
            .map(|backend| backend.listen_addr)
            .min_by_key(|&addr| counts.get(&addr).cloned().unwrap_or(usize::MAX))
            .unwrap();

        info!("Selected least-connection backend: {}", addr);
        addr
    }

    pub async fn update_connection_count(&self, addr: SocketAddr, delta: usize) {
        let mut counts = self.connection_counts.write().await;
        let count = counts.entry(addr).or_insert(0);
        *count = (*count + delta) as usize
    }
}

impl LoadBalancer {
    pub fn new(config: Settings) -> LoadBalancer {
        let rcvr = config.clone().watch_config();
        let mut connection_counts = HashMap::new();
        let mut backend_servers = vec![];

        for backend in config.backends.into_iter() {
            connection_counts.insert(backend.listen_addr, 0);
            backend_servers.push(Arc::new(backend));
        }

        let new_server = Self {
            listen_addr: config.listen_addr,
            proxies: Arc::new(RwLock::new(Proxy {
                backends: backend_servers,
                current_index: Mutex::new(0),
                selector: config.algorithm,
                connection_counts: Arc::new(RwLock::new(connection_counts)),
            })),
            rx: rcvr,
        };
        new_server
    }

    #[tracing::instrument(name = "Update configuration", skip_all, err(Debug))]
    pub async fn update(&mut self, settings: Settings) -> Result<(), Box<dyn std::error::Error>> {
        let mut backend_servers = vec![];

        for backend in settings.backends.into_iter() {
            backend_servers.push(Arc::new(backend))
        }

        let mut proxies = self.proxies.write().await;
        proxies.backends = backend_servers;
        proxies.selector = settings.algorithm;

        Ok(())
    }

    #[tracing::instrument(name = "Sync configuration", skip_all, err(Debug))]
    pub async fn config_sync(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            match self.rx.recv() {
                Ok(new_config) => {
                    info!("New config: {:?}", new_config.algorithm);
                    info!("Backend servers {:?}", new_config.backends);
                    self.update(new_config).await?;
                }
                Err(e) => {
                    error!("watch error: {:?}", e);
                }
            }
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Listening on {}", self.listen_addr);
        let listener: TcpListener = TcpListener::bind(self.listen_addr).await?;

        let proxies = self.proxies.clone();

        tokio::spawn(async move {
            if let Err(e) = run_server(listener, proxies).await {
                error!("Error running proxy server {:?}", e);
                return;
            }
        });
        self.config_sync().await?;

        Ok(())
    }
}

async fn run_server(
    lb_listener: TcpListener,
    proxies: Arc<RwLock<Proxy>>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("LB listener: {:?}", lb_listener);
    loop {
        let (client_stream, _) = lb_listener.accept().await?;
        let proxies_clone = proxies.clone();

        tokio::spawn(async move {
            let backend_addr = {
                let proxies = proxies_clone.read().await;
                proxies.get_next().await
            };

            // update the connection counts
            proxies_clone
                .read()
                .await
                .update_connection_count(backend_addr, 1)
                .await;

            info!("Forwarding connection to {}", backend_addr);
            if let Err(e) = handle_connection(client_stream, backend_addr).await {
                error!("Error running server: {:?}", e);
            }

            // proxies_clone
            //     .read()
            //     .await
            //     .update_connection_count(backend_addr, -1)
            //     .await;
        });
    }
}

async fn handle_connection(
    mut client_stream: TcpStream,
    backend_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut backend_stream = TcpStream::connect(backend_addr).await?;
    match copy_bidirectional(&mut client_stream, &mut backend_stream).await {
        Ok((from_client, from_server)) => {
            debug!(
                "Client wrote {} bytes and received {} bytes",
                from_client, from_server
            );
        }
        Err(e) => {
            error!("Error in connection: {}", e);
        }
    }

    Ok(())
}
