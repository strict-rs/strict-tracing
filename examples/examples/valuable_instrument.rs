//! Example binary for tracing workspace checks.

use std::error::Error;
#[cfg(not(tracing_unstable))]
use std::io::{Write as _, stdout};

/// Application code for the `tracing_unstable` build of this example.
#[cfg(tracing_unstable)]
mod app {
    use std::collections::HashMap;
    use tracing::field::valuable;
    use tracing::{info, instrument};
    use valuable::Valuable;

    /// HTTP headers recorded as a `valuable` instrumentation field.
    #[derive(Valuable)]
    struct Headers<'a> {
        /// Header names and values from the example request.
        headers: HashMap<&'a str, &'a str>,
    }

    /// Process the request headers while recording them through `valuable`.
    // Currently there's no way to automatically apply valuable to a type, so
    // use the fields argument for `instrument`.
    #[instrument(fields(headers=valuable(&headers)))]
    fn process(headers: Headers) {
        info!("Handle request")
    }

    /// Run the unstable `valuable` instrumentation example.
    pub(super) fn run() {
        let headers = HashMap::from([
            ("content-type", "application/json"),
            ("content-length", "568"),
            ("server", "github.com"),
        ]);

        let http_headers = Headers { headers };

        process(http_headers);
    }
}

fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .try_init()?;

    #[cfg(tracing_unstable)]
    app::run();
    #[cfg(not(tracing_unstable))]
    {
        let mut output = stdout();
        writeln!(
            output,
            "Nothing to do, this example needs --cfg=tracing_unstable to run"
        )?;
    };

    Ok(())
}
