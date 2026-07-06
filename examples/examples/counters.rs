//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::io;
use std::io::Write as _;
use std::io::stdout;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use parking_lot::RwLock;
use tracing::Event;
use tracing::Id;
use tracing::Level;
use tracing::Metadata;
use tracing::field::Field;
use tracing::field::Visit;
use tracing::info;
use tracing::span;
use tracing::subscriber;
use tracing::subscriber::Subscriber;
use tracing::subscriber::SubscriberResult;
use tracing::warn;

/// Shared counters keyed by tracing field name.
#[derive(Clone)]
struct Counters(Arc<RwLock<BTreeMap<String, AtomicUsize>>>);

/// Subscriber that increments counters from numeric tracing fields.
struct CounterSubscriber {
  /// Next span identifier assigned by this subscriber.
  ids:      AtomicU64,
  /// Shared counter storage registered from callsite metadata.
  counters: Counters,
}

/// Visitor that applies numeric field values to registered counters.
struct Count<'a> {
  /// Counter map borrowed for the duration of a record operation.
  counters: &'a BTreeMap<String, AtomicUsize>,
}

impl Visit for Count<'_> {
  fn record_i64(&mut self, field: &Field, field_value: i64) {
    let Some(counter) = self.counters.get(field.name()) else {
      return;
    };

    if let Ok(increment) = usize::try_from(field_value) {
      let _previous = counter.fetch_add(increment, Ordering::Release);
      return;
    }

    let magnitude = field_value.unsigned_abs();
    if let Ok(decrement) = usize::try_from(magnitude) {
      let _previous = counter.fetch_sub(decrement, Ordering::Release);
    }
  }

  fn record_u64(&mut self, field: &Field, field_value: u64) {
    if let Some(counter) = self.counters.get(field.name())
      && let Ok(increment) = usize::try_from(field_value)
    {
      let _previous = counter.fetch_add(increment, Ordering::Release);
    }
  }

  fn record_bool(&mut self, _: &Field, _: bool) {}
  fn record_str(&mut self, _: &Field, _: &str) {}
  fn record_debug(&mut self, _: &Field, _: &dyn fmt::Debug) {}
}

impl CounterSubscriber {
  /// Records field values with a short-lived read lock.
  fn record_values(&self, values: &span::Record<'_>) {
    let counters = self.counters.0.read();
    {
      let mut visitor = Count {
        counters: &counters
      };
      values.record(&mut visitor);
    };
    drop(counters);
  }

  /// Records event fields with a short-lived read lock.
  fn record_event(&self, event: &Event<'_>) {
    let counters = self.counters.0.read();
    {
      let mut visitor = Count {
        counters: &counters
      };
      event.record(&mut visitor);
    };
    drop(counters);
  }

  /// Records span attributes with a short-lived read lock.
  fn record_span(&self, new_span: &span::Attributes<'_>) {
    let counters = self.counters.0.read();
    {
      let mut visitor = Count {
        counters: &counters
      };
      new_span.record(&mut visitor);
    };
    drop(counters);
  }
}

impl Subscriber for CounterSubscriber {
  fn register_callsite(&self, meta: &'static Metadata<'static>) -> SubscriberResult<subscriber::Interest> {
    let mut interest = subscriber::Interest::never();
    for key in meta.fields() {
      let name = key.name();
      if name.contains("count") {
        let _counter = self
          .counters
          .0
          .write()
          .entry(name.to_owned())
          .or_insert_with(|| AtomicUsize::new(0));
        interest = subscriber::Interest::always();
      }
    }
    Ok(interest)
  }

  fn new_span(&self, new_span: &span::Attributes<'_>) -> SubscriberResult<Id> {
    self.record_span(new_span);
    let span_id = loop {
      let next_id = self.ids.fetch_add(1, Ordering::SeqCst);
      if let Some(span_id) = Id::try_from_u64(next_id) {
        break span_id;
      }
    };
    Ok(span_id)
  }

  fn record_follows_from(&self, _span: Id, _follows: Id) -> SubscriberResult {
    // unimplemented
    Ok(())
  }

  fn record(&self, _: Id, values: &span::Record<'_>) -> SubscriberResult {
    self.record_values(values);
    Ok(())
  }

  fn event(&self, event: &Event<'_>) -> SubscriberResult {
    self.record_event(event);
    Ok(())
  }

  fn enabled(&self, metadata: &Metadata<'_>) -> SubscriberResult<bool> {
    Ok(metadata.fields().iter().any(|f| f.name().contains("count")))
  }

  fn enter(&self, _span: Id) -> SubscriberResult {
    Ok(())
  }

  fn exit(&self, _span: Id) -> SubscriberResult {
    Ok(())
  }
}

impl Counters {
  /// Writes counters in deterministic field-name order.
  fn print_counters(&self) -> io::Result<()> {
    let mut output = stdout();
    {
      let counters = self.0.read();
      for (counter_name, counter_value) in counters.iter() {
        writeln!(output, "{counter_name}: {}", counter_value.load(Ordering::Acquire))?;
      }
    }
    Ok(())
  }

  /// Creates shared counters and their subscriber.
  #[allow(
    clippy::single_call_fn,
    reason = "pairs the shared counter storage with its subscriber for readable setup"
  )]
  fn new() -> (Self, CounterSubscriber) {
    let counters = Self(Arc::new(RwLock::new(BTreeMap::new())));
    let subscriber = CounterSubscriber {
      ids:      AtomicU64::new(1),
      counters: counters.clone(),
    };
    (counters, subscriber)
  }
}

fn main() -> Result<(), Box<dyn Error>> {
  let (counters, subscriber) = Counters::new();

  subscriber::set_global_default(subscriber)?;

  let mut foo_count: u64 = 2;
  span!(Level::TRACE, "my_great_span", foo_count = &foo_count).in_scope(|| {
    foo_count = foo_count.saturating_add(1);
    info!(yak_shaved = true, yak_count = 1, "hi from inside my span");
    span!(Level::TRACE, "my other span", foo_count = &foo_count, baz_count = 5).in_scope(|| {
      warn!(yak_shaved = false, yak_count = -1, "failed to shave yak");
    });
  });

  counters.print_counters()?;
  Ok(())
}
