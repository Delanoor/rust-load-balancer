use std::net::SocketAddr;

use rust_load_balancer::proxy::{Server, Service};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let services = vec![Service::new(
        "example".to_string(),
        "127.0.0.1:3000".parse().unwrap(),
        vec![
            "127.0.0.1:8000".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
        ],
    )];

    let mut server = Server::new(services)?;
    server.run().await?;

    Ok(())
}
