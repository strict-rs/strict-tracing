//! `Dispatch::get_default` clone benchmarks.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

mod shared;

fn bench(c: &mut Criterion) {
    shared::for_all_dispatches(&mut c.benchmark_group("Dispatch::get_clone"), |b| {
        b.iter(|| {
            let current = tracing::dispatcher::get_default(|current| current.clone());
            let _value = black_box(current);
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
