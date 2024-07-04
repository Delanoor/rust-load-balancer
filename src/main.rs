use std::sync::Arc;

use rust_load_balancer::configuration::Settings;
use rust_load_balancer::proxy::Server;
use rust_load_balancer::utils::tracing::init_tracing;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    color_eyre::install().expect("Failed to install color_eyre");
    init_tracing().expect("Failed to initialize tracing");

    let configs = Settings::new().expect("Failed to load configuration.");

    let server = Server::new(configs.listen_addr, configs.backends);

    Arc::new(server).run().await
}
