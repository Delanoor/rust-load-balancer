use rand::Rng;
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::debug;

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
}

impl Proxy {
    pub async fn get_next(&self) -> SocketAddr {
        match self.selector {
            LoadBalancingAlgorithm::Random => {
                let mut rng = rand::thread_rng();
                let random_index = rng.gen_range(0..self.backends.len());
                tracing::info!("New random number: {}", random_index);
                self.backends[random_index].listen_addr
            }
            LoadBalancingAlgorithm::RoundRobin => {
                let mut index = self.current_index.lock().unwrap();
                let addr = self.backends[*index].listen_addr;
                *index = (*index + 1) % self.backends.len();
                addr
            }
        }
    }
}

impl LoadBalancer {
    pub fn new(config: Settings) -> LoadBalancer {
        let rcvr = config.clone().watch_config();

        let mut backend_servers = vec![];

        for backend in config.backends.into_iter() {
            backend_servers.push(Arc::new(backend))
        }

        let new_server = Self {
            listen_addr: config.listen_addr,
            proxies: Arc::new(RwLock::new(Proxy {
                backends: backend_servers,
                current_index: Mutex::new(0),
                selector: config.algorithm,
            })),

            rx: rcvr,
        };
        // Arc::new(new_server)
        new_server
    }

    pub async fn update(&mut self, settings: Settings) {
        let mut backend_servers = vec![];

        for backend in settings.backends.into_iter() {
            backend_servers.push(Arc::new(backend))
        }

        let mut proxies = self.proxies.write().await;
        proxies.backends = backend_servers;
        proxies.selector = settings.algorithm;
    }

    // wait on config changes to update backend server pool
    pub async fn config_sync(&mut self) {
        loop {
            match self.rx.recv() {
                Ok(new_config) => {
                    tracing::info!("Config file watch event. New config: {:?}", new_config);

                    self.update(new_config).await;
                }
                Err(e) => {
                    tracing::error!("watch error: {:?}", e);
                    break;
                }
            }
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("Listening on {}", self.listen_addr);
        let listener: TcpListener = TcpListener::bind(self.listen_addr).await?;

        let proxies = self.proxies.clone();

        tokio::spawn(async move {
            if let Err(e) = run_server(listener, proxies).await {
                tracing::error!("Error running proxy server {:?}", e);
                return;
            }
        });
        self.config_sync().await;

        Ok(())
    }
}
async fn run_server(
    lb_listener: TcpListener,
    proxies: Arc<RwLock<Proxy>>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("LB listener: {:?}", lb_listener);
    loop {
        let (client_stream, _) = lb_listener.accept().await?;
        let proxies_clone = proxies.clone();

        tokio::spawn(async move {
            let backend_addr = {
                let proxies = proxies_clone.read().await;
                proxies.get_next().await
            };

            tracing::info!("Forwarding connection to {}", backend_addr);
            if let Err(e) = handle_connection(client_stream, backend_addr).await {
                tracing::error!("Error running server: {:?}", e);
            }
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
            tracing::error!("Error in connection: {}", e);
        }
    }

    // client_stream.shutdown().await?;
    // backend_stream.shutdown().await?;
    Ok(())
}
