### In Applications

In order to record trace events, executables have to use a `Subscriber`
implementation compatible with `tracing`. A `Subscriber` implements a way of
collecting trace data, such as by logging it to standard output.
[`tracing-subscriber`][tracing-subscriber-docs]'s [`fmt` module][fmt] provides
a subscriber for logging traces with reasonable defaults. Additionally,
`tracing-subscriber` is able to consume messages emitted by `log`-instrumented
libraries and modules.

To use `tracing-subscriber`, add the following to your `Cargo.toml`:

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = "0.3"
```

Then create and install a `Subscriber`, for example using [`try_init()`]:

```rust
use std::error::Error;
use tracing::info;
use tracing_subscriber;

fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    // install global subscriber configured based on RUST_LOG envvar.
    tracing_subscriber::fmt().try_init()?;

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
```

Using `try_init()` calls [`set_global_default()`] and returns an error if a
global subscriber has already been installed. Once installed successfully, this
subscriber will be used as the default in all threads for the remainder of the
duration of the program, similar to how loggers work in the `log` crate.

[tracing-subscriber-docs]: https://docs.rs/tracing-subscriber/
[fmt]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html
[`set_global_default`]: https://docs.rs/tracing/latest/tracing/subscriber/fn.set_global_default.html


For more control, a subscriber can be built in stages and not set globally,
but instead used to locally override the default subscriber. For example:

```rust
use tracing::{info, Level};
use tracing_subscriber;

fn main() {
    let subscriber = tracing_subscriber::fmt()
        // filter spans/events with level TRACE or higher.
        .with_max_level(Level::TRACE)
        // build but do not install the subscriber.
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        info!("This will be logged to stdout");
    });
    info!("This will _not_ be logged to stdout");
}
```

Any trace events generated outside the context of a subscriber will not be collected.

This approach allows trace data to be collected by multiple subscribers
within different contexts in the program. Note that the override only applies to the
currently executing thread; other threads will not see the change from with_default.

Once a subscriber has been set, instrumentation points may be added to the
executable using the `tracing` crate's macros.

[`tracing-subscriber`]: https://docs.rs/tracing-subscriber/
[fmt]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html
[`try_init()`]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/fn.try_init.html
[`set_global_default()`]: https://docs.rs/tracing/latest/tracing/subscriber/fn.set_global_default.html
