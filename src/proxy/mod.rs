use rand::Rng;
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use tokio::io::{copy_bidirectional, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::debug;

pub mod backend;
use crate::configuration::{LoadBalancingAlgorithm, Settings};
use backend::Backend;

#[derive(Debug)]
pub struct Server {
    pub listen_addr: SocketAddr,
    pub backends: Vec<Backend>,
    pub current_index: Mutex<usize>,
    pub selector: LoadBalancingAlgorithm,
    rx: Receiver<Settings>,
}

impl Server {
    pub fn new(config: Settings) -> Server {
        let rcvr = config.clone().watch_config();

        let new_server = Self {
            listen_addr: config.listen_addr,
            backends: config.backends,
            current_index: Mutex::new(0),
            selector: config.algorithm,
            rx: rcvr,
        };
        // Arc::new(new_server)
        new_server
    }

    pub fn update(&mut self, settings: Settings) {
        self.backends = settings.backends;
        self.selector = settings.algorithm;
    }

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

    // wait on config changes to update backend server pool
    pub async fn config_sync(&mut self) {
        loop {
            match self.rx.recv() {
                Ok(new_config) => {
                    tracing::info!("Config file watch event. New config: {:?}", new_config);

                    self.update(new_config);
                }
                Err(e) => {
                    tracing::error!("watch error: {:?}", e);
                    break;
                }
            }
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(&self.listen_addr).await?;
        tracing::info!("Listening on {}", self.listen_addr);

        self.config_sync().await;
        loop {
            let (client_stream, _) = listener.accept().await?;
            let backend_addr = self.get_next().await;
            tracing::info!("Forwarding connection to {}", backend_addr);

            tokio::spawn(async move {
                if let Err(e) = handle_connection(client_stream, backend_addr).await {
                    tracing::error!("Error running server: {:?}", e);
                }
            });
        }
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

    client_stream.shutdown().await?;
    backend_stream.shutdown().await?;

    Ok(())
}
