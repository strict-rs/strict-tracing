//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]
use std::io::stderr;
use tracing::{error, subscriber::with_default};
use tracing_subscriber::fmt;

fn main() {
    let subscriber = fmt().with_writer(stderr).finish();

    with_default(subscriber, || {
        error!("This event will be printed to `stderr`.");
    });
}
