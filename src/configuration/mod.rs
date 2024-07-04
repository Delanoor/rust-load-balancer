use config::{Config, ConfigError, File};
use serde::Deserialize;
use std::{env, net::SocketAddr};

use crate::proxy::backend::Backend;

#[derive(Debug, Deserialize)]
pub struct RawSettings {
    pub listen_addr: String,
    pub backends: Vec<RawBackend>,
}

#[derive(Debug, Deserialize)]
pub struct RawBackend {
    pub name: String,
    pub addr: String,
}

#[derive(Debug)]
pub struct Settings {
    pub listen_addr: SocketAddr,
    pub backends: Vec<Backend>,
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

        Ok(Self {
            listen_addr,
            backends,
        })
    }
}
