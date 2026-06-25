//! Example binary for tracing workspace checks.
use std::{
    error::Error,
    fmt,
    io::{Write as _, stdout},
    sync::atomic::{AtomicU64, Ordering},
};

use tracing::{Level, debug, error, info, span, subscriber::with_default, trace, warn};
use tracing_core::{
    event::Event,
    metadata::Metadata,
    span::{Attributes, Id, Record},
    subscriber::{Subscriber, SubscriberResult},
};
use tracing_serde::AsSerde as _;

use serde_json::{Value, json};

/// Subscriber that writes each callback as a JSON object.
#[derive(Debug)]
struct JsonSubscriber {
    /// Next span identifier assigned by this subscriber.
    next_id: AtomicU64,
}

impl Subscriber for JsonSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> SubscriberResult<bool> {
        let payload = json!({
        "enabled": {
            "metadata": metadata.as_serde(),
        }});
        write_json(&payload);
        Ok(true)
    }

    fn new_span(&self, attrs: &Attributes<'_>) -> SubscriberResult<Id> {
        let span_id = loop {
            let next_id = self.next_id.fetch_add(1, Ordering::Relaxed);
            if let Some(span_id) = Id::try_from_u64(next_id) {
                break span_id;
            }
        };
        let payload = json!({
        "new_span": {
            "attributes": attrs.as_serde(),
            "id": span_id.as_serde(),
        }});
        write_json(&payload);
        Ok(span_id)
    }

    fn record(&self, span: Id, values: &Record<'_>) -> SubscriberResult {
        let payload = json!({
        "record": {
            "span": span.as_serde(),
            "values": values.as_serde(),
        }});
        write_json(&payload);
        Ok(())
    }

    fn record_follows_from(&self, span: Id, follows: Id) -> SubscriberResult {
        let payload = json!({
        "record_follows_from": {
            "span": span.as_serde(),
            "follows": follows.as_serde(),
        }});
        write_json(&payload);
        Ok(())
    }

    fn event(&self, event: &Event<'_>) -> SubscriberResult {
        let payload = json!({
            "event": event.as_serde(),
        });
        write_json(&payload);
        Ok(())
    }

    fn enter(&self, span: Id) -> SubscriberResult {
        let payload = json!({
            "enter": span.as_serde(),
        });
        write_json(&payload);
        Ok(())
    }

    fn exit(&self, span: Id) -> SubscriberResult {
        let payload = json!({
            "exit": span.as_serde(),
        });
        write_json(&payload);
        Ok(())
    }
}

/// Writes one serialized subscriber callback to standard output.
fn write_json(payload: &Value) {
    let mut output = stdout();
    let _write_result = writeln!(output, "{payload}");
}

/// Error returned when a yak is missing.
#[derive(Debug)]
struct MissingYak;

impl fmt::Display for MissingYak {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("missing yak")
    }
}

impl Error for MissingYak {}

/// Emits tracing events for shaving a single yak.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the per-yak instrumented operation distinct from the aggregate workflow"
)]
#[tracing::instrument]
fn shave(yak: usize) -> Result<(), MissingYak> {
    trace!(excitement = "yay!", "hello! I'm gonna shave a yak");
    if yak == 3 {
        warn!("could not locate yak");
        return Err(MissingYak);
    }
    trace!("yak shaved successfully");
    Ok(())
}

/// Emits tracing events while shaving all requested yaks.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the aggregate yak-shaving workflow named for the serde subscriber demo"
)]
fn shave_all(yaks: usize) -> usize {
    let span = span!(Level::INFO, "shaving_yaks", yaks);
    let _enter = span.enter();

    info!("shaving yaks");

    let mut yaks_shaved: usize = 0;
    for yak in 1..=yaks {
        let shave_result = shave(yak);
        debug!(target: "yak_events", yak, shaved = shave_result.is_ok());

        if let Err(error) = shave_result {
            let error_ref: &dyn Error = &error;
            error!(yak, error = error_ref, "failed to shave yak");
        } else {
            yaks_shaved = yaks_shaved.saturating_add(1);
        }
        trace!(yaks_shaved);
    }

    yaks_shaved
}

fn main() {
    let subscriber = JsonSubscriber {
        next_id: AtomicU64::new(1),
    };

    with_default(subscriber, || {
        let number_of_yaks = 3;
        debug!("preparing to shave {} yaks", number_of_yaks);

        let number_shaved = shave_all(number_of_yaks);

        debug!(
            message = "yak shaving completed.",
            all_yaks_shaved = number_shaved == number_of_yaks,
        );
    });
}
