use config::{Config, ConfigError, File};
use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

use std::{env, net::SocketAddr};

use crate::proxy::backend::Backend;

#[derive(Debug, Deserialize)]
pub struct RawSettings {
    pub listen_addr: String,
    pub backends: Vec<RawBackend>,
    pub algorithm: LoadBalancingAlgorithm,
}

#[derive(Debug, Deserialize, Clone)]
pub enum LoadBalancingAlgorithm {
    #[serde(rename = "round-robin")]
    RoundRobin,
    #[serde(rename = "random")]
    Random,
}

#[derive(Debug, Deserialize)]
pub struct RawBackend {
    pub name: String,
    pub addr: String,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub listen_addr: SocketAddr,
    pub backends: Vec<Backend>,
    pub algorithm: LoadBalancingAlgorithm,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
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

        Ok(Self {
            listen_addr,
            backends,
            algorithm,
        })
    }

    pub fn watch_config(self) -> Result<Receiver<Settings>, ConfigError> {
        let (tx, rx) = channel();
        let config_tx = tx.clone();

        let mut watcher: RecommendedWatcher = recommended_watcher(move |res| match res {
            Ok(event) => match Settings::new() {
                Ok(new_settings) => {
                    if let Err(e) = config_tx.send(new_settings) {
                        println!("Error sending new config: {:?}", e);
                    }
                }
                Err(e) => println!("Error reloading config: {:?}", e),
            },
            Err(e) => println!("Watch error: {:?}", e),
            _ => {}
        })
        .unwrap();

        watcher
            .watch(Path::new("config.toml"), RecursiveMode::NonRecursive)
            .unwrap();

        Ok(rx)
    }
}
