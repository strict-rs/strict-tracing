//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]
#[path = "fmt/yak_shave.rs"]
pub mod yak_shave;

use std::error::Error;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    use tracing_subscriber::fmt;

    // Configure a custom event formatter
    let format = fmt::format()
        .compact() // use an abbreviated format for logging spans
        .with_level(false) // don't include levels in formatted output
        .with_target(false); // don't include targets

    // Create a `fmt` subscriber that uses our custom event format, and set it
    // as the default.
    fmt().event_format(format).try_init()?;

    // Shave some yaks!
    let number_of_yaks = 3;
    // this creates a new event, outside of any spans.
    tracing::info!(number_of_yaks, "preparing to shave yaks");

    let number_shaved = yak_shave::shave_all(number_of_yaks);
    tracing::info!(
        all_yaks_shaved = number_shaved == number_of_yaks,
        "yak shaving completed."
    );
    Ok(())
}
