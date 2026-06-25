//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]
/// This is a example showing how thread info can be displayed when
/// formatting events with `tracing_subscriber::fmt`. This is useful
/// as `tracing` spans can be entered by multicple threads concurrently,
/// or move across threads freely.
///
/// You can run this example by running the following command in a terminal
///
/// ```
/// cargo run --example thread-info
/// ```
///
/// Example output:
///
/// ```not_rust
/// Jul 17 00:38:07.177  INFO ThreadId(02) thread_info: i=9
/// Jul 17 00:38:07.177  INFO            thread 1 ThreadId(03) thread_info: i=9
/// Jul 17 00:38:07.177  INFO large name thread 2 ThreadId(04) thread_info: i=9
/// ```
use std::error::Error;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tracing::info;

/// Wait for a worker thread and report panics as ordinary example errors.
fn wait_for_thread(handle: JoinHandle<()>) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if handle.join().is_err() {
        return Err("worker thread panicked".into());
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        // enable thread id to be emitted
        .with_thread_ids(true)
        // enabled thread name to be emitted
        .with_thread_names(true)
        .try_init()?;

    let do_work = || {
        for i in 1..10 {
            info!(i);
            thread::sleep(Duration::from_millis(1));
        }
    };

    let thread_with_no_name = thread::spawn(do_work);
    let thread_one = thread::Builder::new()
        .name("thread 1".to_owned())
        .spawn(do_work)?;
    let thread_two = thread::Builder::new()
        .name("large name thread 2".to_owned())
        .spawn(do_work)?;

    wait_for_thread(thread_with_no_name)?;
    wait_for_thread(thread_one)?;
    wait_for_thread(thread_two)?;

    Ok(())
}
