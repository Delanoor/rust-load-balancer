use std::{net::SocketAddr, time::Duration};

use serde::Deserialize;
use tokio::{net::TcpStream, time::timeout};

#[derive(Debug, Clone, Deserialize)]
pub struct Backend {
    pub name: String,
    pub listen_addr: SocketAddr,
}

impl Backend {
    pub fn new(name: String, listen_addr: SocketAddr) -> Backend {
        Backend { name, listen_addr }
    }

    pub async fn health_check(&self) -> bool {
        match timeout(Duration::from_secs(2), TcpStream::connect(self.listen_addr)).await {
            Ok(Ok(_)) => true, // Connection succeeded
            _ => false,
        }
    }
}
