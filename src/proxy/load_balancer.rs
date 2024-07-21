use std::collections::HashMap;
use std::net::SocketAddr;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{error, info};

use crate::proxy::proxy::Proxy;

use crate::configuration::{LoadBalancingAlgorithm, Settings};

#[derive(Debug)]
pub struct LoadBalancer {
    pub listen_addr: SocketAddr,
    pub proxies: Arc<RwLock<Proxy>>,
    rx: mpsc::Receiver<Settings>,
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
                healthy_backends: RwLock::new(Vec::new()),
            })),
            rx: rcvr,
            monitoring_interval: Duration::from_secs(config.monitoring_interval),
            health_check_interval: Duration::from_secs(config.health_check_interval),
        };

        new_server
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

    #[tracing::instrument(name = "Run monitoring", skip_all, err(Debug))]
    pub async fn run_monitoring(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            let start_time = Instant::now();

            let response_times = self.collect_metrics().await?;

            self.check_conditions_and_update(response_times).await?;

            let elapsed_time = start_time.elapsed();

            // check if time left
            if elapsed_time < self.monitoring_interval {
                sleep(self.monitoring_interval - elapsed_time).await;
            }
        }
    }

    async fn collect_metrics(&self) -> Result<Vec<Duration>, Box<dyn std::error::Error>> {
        let proxies = self.proxies.read().await;
        let mut response_times = Vec::new();

        for backend in &proxies.backends {
            let start = Instant::now();
            let result = backend.health_check().await;
            let duration = start.elapsed();

            if result {
                response_times.push(duration);
            } else {
                //  push a large duration to indicate a failure.
                //  TODO
                response_times.push(Duration::from_secs(9999));
            }
        }

        Ok(response_times)
    }

    async fn check_conditions_and_update(
        &self,
        response_times: Vec<Duration>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let average_response_time: Duration =
            response_times.iter().sum::<Duration>() / response_times.len() as u32;
        if average_response_time > Duration::from_millis(150) {
            self.update_algorithm(LoadBalancingAlgorithm::LeastConnection)
                .await?;
        } else {
            self.update_algorithm(LoadBalancingAlgorithm::RoundRobin)
                .await?;
        }

        Ok(())
    }

    async fn update_algorithm(
        &self,
        new_algorithm: LoadBalancingAlgorithm,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut proxies = self.proxies.write().await;

        proxies.selector = new_algorithm;

        Ok(())
    }

    async fn run_health_checks(&self) -> Result<(), Box<dyn std::error::Error>> {
        let start_time = Instant::now();

        {
            let proxies = self.proxies.read().await;
            proxies.update_healthy_backends().await;
        }
        let elapsed_time = start_time.elapsed();
        if elapsed_time < self.health_check_interval {
            sleep(self.health_check_interval - elapsed_time).await;
        }

        Ok(())
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
        while let Some(new_config) = self.rx.recv().await {
            info!("New load balancing algorithm: {:?}", new_config.algorithm);
            info!("Backend servers {:?}", new_config.backends);
            info!(
                "Health check interval: {:?}",
                new_config.health_check_interval
            );
            info!(
                "Monitoring check interval: {:?}",
                new_config.monitoring_interval
            );
            self.update(new_config).await?;
        }

        Ok(())
    }

    #[tracing::instrument(name = "Run monitoring and health checks", skip_all, err(Debug))]
    async fn monitor_and_health_check(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut monitoring_interval = tokio::time::interval(self.monitoring_interval);
        let mut health_check_interval = tokio::time::interval(self.health_check_interval);

        loop {
            tokio::select! {
                _ = health_check_interval.tick() =>{
                    self.run_health_checks().await?;
                }

                _ = monitoring_interval.tick() => {
                    self.run_monitoring().await?;
                }
            }
        }
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
