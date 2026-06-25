//! Shared yak-shaving workload for the formatting examples.

use snafu::{ResultExt as _, Snafu};
use std::error::Error;
use thiserror::Error;
use tracing::{Level, debug, error, info, span, trace, warn};

// the `#[tracing::instrument]` attribute creates and enters a span
// every time the instrumented function is called. The span is named after the
// the function or method. Paramaters passed to the function are recorded as fields.
/// Shaves one yak and records the span and events emitted along the way.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the per-yak instrumented operation distinct from the aggregate workflow"
)]
#[tracing::instrument]
fn shave(yak: usize) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    // this creates an event at the TRACE log level with two fields:
    // - `excitement`, with the key "excitement" and the value "yay!"
    // - `message`, with the key "message" and the value "hello! I'm gonna shave a yak."
    //
    // unlike other fields, `message`'s shorthand initialization is just the string itself.
    trace!(excitement = "yay!", "hello! I'm gonna shave a yak");
    if yak == 3 {
        warn!("could not locate yak");
        return OutOfCashSnafu
            .fail()
            .map_err(|source| MissingYakError::OutOfSpace { source })
            .context(MissingYakSnafu)
            .map_err(Into::into);
    }
    trace!("yak shaved successfully");
    Ok(())
}

/// Shaves `yaks` yaks and returns the number that completed successfully.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the shared yak-shaving workload named across formatting examples"
)]
pub(crate) fn shave_all(yaks: usize) -> usize {
    // Constructs a new span named "shaving_yaks" at the INFO level,
    // and a field whose key is "yaks". This is equivalent to writing:
    //
    // let span = span!(Level::INFO, "shaving_yaks", yaks = yaks);
    //
    // local variables (`yaks`) can be used as field values
    // without an assignment, similar to struct initializers.
    let span = span!(Level::INFO, "shaving_yaks", yaks);
    let _enter = span.enter();

    info!("shaving yaks");

    let mut yaks_shaved = 0_usize;
    for yak in 1..=yaks {
        let res = shave(yak);
        debug!(target: "yak_events", yak, shaved = res.is_ok());

        if let Err(ref error) = res {
            // Like spans, events can also use the field initialization shorthand.
            // In this instance, `yak` is the field being initalized.
            error!(yak, error = error.as_ref(), "failed to shave yak");
        } else {
            yaks_shaved = yaks_shaved.saturating_add(1);
        }
        trace!(yaks_shaved);
    }

    yaks_shaved
}

// Usually you would pick one error handling library to use, but they can be mixed freely.
/// Error type used to show a `snafu` source in formatted events.
#[derive(Debug, Snafu)]
enum OutOfSpaceError {
    /// Indicates that the yak-shaving budget ran out.
    #[snafu(display("out of cash"))]
    OutOfCash,
}

/// Error type used to show a `thiserror` source in formatted events.
#[derive(Debug, Error)]
enum MissingYakError {
    /// Indicates that the selected yak could not be prepared.
    #[error("out of space")]
    OutOfSpace {
        /// The lower-level yak preparation error.
        source: OutOfSpaceError,
    },
}

/// Top-level yak-shaving error emitted by this example.
#[derive(Debug, Snafu)]
enum YakError {
    /// Indicates that a yak was unavailable.
    #[snafu(display("missing yak"))]
    MissingYak {
        /// The reason the yak was unavailable.
        source: MissingYakError,
    },
}
