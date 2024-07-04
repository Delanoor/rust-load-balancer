use color_eyre::eyre::Result;

use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter};

pub fn init_tracing() -> Result<()> {
    let fmt_layer = fmt::layer().pretty();

    // Create a filter layer to control the verbosity of logs
    // Try to get the filter config from the environment variables
    // if fails, default to "info" log level

    let filter_layer: EnvFilter =
        EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new(""))?;

    // Build the tracing subscriber registry with the formatting layer, filter layer,
    // and the error layer for enhanced error reporting

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(ErrorLayer::default())
        .init();

    Ok(())
}
