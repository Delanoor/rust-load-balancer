use rust_load_balancer::configuration::Settings;
use rust_load_balancer::proxy::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let configs = Settings::new().expect("Failed to load configuration.");

    let server = Server::new(configs.listen_addr, configs.backends);

    server.run().await?;

    Ok(())
}
