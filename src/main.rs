use app::configuration::Settings;
use app::proxy::Server;
use app::utils::tracing::init_tracing;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    color_eyre::install().expect("Failed to install color_eyre");
    init_tracing().expect("Failed to initialize tracing");

    let configs = Settings::load().expect("Failed to load configuration.");

    let mut lb = Server::new(configs);

    if let Err(_) = lb.run().await {
        tracing::error!("Failed to start lb")
    }

    Ok(())
}
