use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{error, info};

use crate::proxy::proxy::Proxy;

use crate::configuration::Settings;

#[derive(Debug)]
pub struct LoadBalancer {
    pub listen_addr: SocketAddr,
    pub proxies: Arc<RwLock<Proxy>>,
    rx: Receiver<Settings>,
    monitoring_interval: Duration,
    health_check_interval: Duration,
}

impl LoadBalancer {
    pub fn new(config: Settings) -> Self {
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
            monitoring_interval: Duration::from_secs(config.monitoring_interval),
            health_check_interval: Duration::from_secs(config.health_check_interval),
        };

        new_server
    }

    #[tracing::instrument(name = "Run monitoring", skip_all, err(Debug))]
    pub async fn run_monitoring(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            let start_time = Instant::now();

            let response_times = self.collect_metrics().await?;

            self.check_conditions_and_update(response_times).await?;

            let elapsed_time = start_time.elapsed();

            if elapsed_time < self.monitoring_interval {
                sleep(self.monitoring_interval - elapsed_time).await;
            }
        }
    }

    async fn collect_metrics(&self) -> Result<Vec<Duration>, Box<dyn std::error::Error>> {
        todo!()
    }

    async fn check_conditions_and_update(
        &self,
        response_times: Vec<Duration>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        todo!()
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

        self.monitoring_interval = Duration::from_secs(settings.monitoring_interval);
        self.health_check_interval = Duration::from_secs(settings.health_check_interval);

        Ok(())
    }

    #[tracing::instrument(name = "Sync configuration", skip_all, err(Debug))]
    pub async fn config_sync(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            match self.rx.recv() {
                Ok(new_config) => {
                    info!("New load balancing algorithm: {:?}", new_config.algorithm);
                    info!("Backend servers {:?}", new_config.backends);
                    info!(
                        "Health check interva; {:?}",
                        new_config.health_check_interval
                    );
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

        // tokio::spawn(async move {
        //     if let Err(e) = self.
        // });

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
    loop {
        let (client_stream, _) = lb_listener.accept().await?;
        let proxies_clone = proxies.clone();

        tokio::spawn(async move {
            let backend_addr = {
                let proxies = proxies_clone.read().await;
                proxies.get_next().await
            };

            // increment the connection counts
            proxies_clone
                .read()
                .await
                .update_connection_count(backend_addr, 1)
                .await;

            info!("Forwarding connection to {}", backend_addr);

            match handle_connection(client_stream, backend_addr).await {
                Ok(_) => tracing::info!("Connection to {} closed", backend_addr),
                Err(e) => tracing::error!("Error handling connection to {}: {:?}", backend_addr, e),
            }

            proxies_clone
                .read()
                .await
                .update_connection_count(backend_addr, -1)
                .await;
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
            tracing::info!(
                "Client wrote {} bytes and received {} bytes",
                from_client,
                from_server
            );
        }
        Err(e) => {
            tracing::error!("Error in connection: {}", e);
        }
    }

    Ok(())
}
