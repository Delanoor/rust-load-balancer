use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct Backend {
    pub name: String,
    pub addr: SocketAddr,
}
