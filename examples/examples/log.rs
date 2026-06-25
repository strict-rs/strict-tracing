//! Example binary for tracing workspace checks.

use std::error::Error;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .try_init()?;

    log::debug!("this is a log line");
    tracing::debug!("this is a tracing line");
    Ok(())
}
