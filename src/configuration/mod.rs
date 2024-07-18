use config::{Config, ConfigError, File};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::{env, net::SocketAddr};
use tracing::error;

use crate::proxy::backend::Backend;

#[derive(Debug, Deserialize)]
pub struct RawSettings {
    pub listen_addr: String,
    pub backends: Vec<RawBackend>,
    pub algorithm: LoadBalancingAlgorithm,
    pub monitoring_interval: u64,
    pub health_check_interval: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub enum LoadBalancingAlgorithm {
    #[serde(rename = "round-robin")]
    RoundRobin,
    #[serde(rename = "random")]
    Random,
    #[serde(rename = "least-connection")]
    LeastConnection,
}

#[derive(Debug, Deserialize)]
pub struct RawBackend {
    pub name: String,
    pub addr: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub listen_addr: SocketAddr,
    pub backends: Vec<Backend>,
    pub algorithm: LoadBalancingAlgorithm,
    pub monitoring_interval: u64,
    pub health_check_interval: u64,
}

impl Settings {
    pub fn load() -> Result<Self, ConfigError> {
        let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "development".into());
        let builder = Config::builder()
            .add_source(File::with_name(&format!("{}", run_mode)).required(false))
            .add_source(File::with_name("config").required(run_mode == "production"))
            .build()?;

        let raw: RawSettings = builder.try_deserialize()?;

        let listen_addr = raw.listen_addr.parse().expect("Invalid listen address");
        let backends = raw
            .backends
            .into_iter()
            .map(|backend| Backend {
                name: backend.name,
                listen_addr: backend.addr.parse().expect("Invalid backend address"),
            })
            .collect();

        let algorithm = raw.algorithm;
        let monitoring_interval = raw.monitoring_interval;
        let health_check_interval = raw.health_check_interval;

        Ok(Self {
            listen_addr,
            backends,
            algorithm,
            monitoring_interval,
            health_check_interval,
        })
    }

    pub fn watch_config(self) -> Receiver<Settings> {
        let (config_tx, config_rx) = channel();

        thread::spawn(move || {
            let (tx, rx) = channel();
            let mut watcher = match RecommendedWatcher::new(tx, notify::Config::default()) {
                Ok(w) => w,
                Err(e) => {
                    error!("Error creating watcher: {:?}", e);
                    return;
                }
            };

            if let Err(e) =
                watcher.watch(Path::new("development.toml"), RecursiveMode::NonRecursive)
            {
                error!("Error watching file: {:?}", e);
                return;
            }

            loop {
                match rx.recv() {
                    Ok(_) => match Settings::load() {
                        Ok(new_settings) => {
                            if let Err(e) = config_tx.send(new_settings) {
                                error!("Error sending reloaded config: {:?}", e);
                            }
                        }
                        Err(e) => {
                            error!("Error reloading config: {:?}", e);
                        }
                    },
                    Err(e) => {
                        error!("Error receiving file change event: {:?}", e);
                        break;
                    }
                }
            }
        });

        config_rx
    }
}
