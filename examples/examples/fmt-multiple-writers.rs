//! An example demonstrating how `fmt::Layer` can write to multiple
//! destinations (in this instance, `stdout` and a file) simultaneously.

#[path = "fmt/yak_shave.rs"]
pub mod yak_shave;

use std::{error::Error, io::stdout};
use tracing::{Level, info, subscriber::set_global_default};
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt as _, registry};

fn main() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;

    let file_appender = rolling::hourly(dir.path(), "example.log")?;
    let (file_writer, _guard) = non_blocking(file_appender);

    let subscriber = registry()
        .with(EnvFilter::from_default_env().add_directive(Level::TRACE.into()))
        .with(fmt::Layer::new().with_writer(stdout))
        .with(fmt::Layer::new().with_writer(file_writer));
    set_global_default(subscriber)?;

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
