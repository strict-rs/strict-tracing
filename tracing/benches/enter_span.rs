//! Span enter benchmarks.

use criterion::{Criterion, criterion_group, criterion_main};
use tracing::{Level, span};

mod shared;

fn bench(c: &mut Criterion) {
    shared::for_all_dispatches(&mut c.benchmark_group("enter_span"), |b| {
        let span = span!(Level::TRACE, "span");
        b.iter(|| {
            let _span = span.enter();
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
