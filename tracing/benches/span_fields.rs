//! Span field benchmarks.

use criterion::{Criterion, criterion_group, criterion_main};
use tracing::{Level, span};

mod shared;

fn bench(c: &mut Criterion) {
    shared::for_all_recording(&mut c.benchmark_group("span_fields"), |b| {
        b.iter(|| {
            let span = span!(
                Level::TRACE,
                "span",
                foo = "foo",
                bar = "bar",
                baz = 3,
                quuux = tracing::field::debug(0.99)
            );
            std::hint::black_box(span)
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
