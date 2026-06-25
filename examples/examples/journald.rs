//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]
use std::error::Error;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*};

/// Shared yak-shaving helper used by this example.
#[path = "fmt/yak_shave.rs"]
pub mod yak_shave;

fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let registry = tracing_subscriber::registry().with(fmt::layer().with_target(false));
    match tracing_journald::layer() {
        Ok(layer) => {
            registry.with(layer).try_init()?;
        }
        // journald is typically available on Linux systems, but nowhere else. Portable software
        // should handle its absence gracefully.
        Err(error) => {
            registry.try_init()?;
            error!("couldn't connect to journald: {}", error);
        }
    }

    let number_of_yaks = 3;
    // this creates a new event, outside of any spans.
    info!(number_of_yaks, "preparing to shave yaks");

    let number_shaved = yak_shave::shave_all(number_of_yaks);
    info!(
        all_yaks_shaved = number_shaved == number_of_yaks,
        "yak shaving completed."
    );

    Ok(())
}
