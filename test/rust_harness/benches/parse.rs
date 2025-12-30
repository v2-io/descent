//! Benchmarks for descent-generated parsers.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use descent_harness::Parser;

fn bench_parse(c: &mut Criterion) {
    let input = b"hello world";

    c.bench_function("parse_minimal", |b| {
        b.iter(|| {
            let mut count = 0usize;
            Parser::new(black_box(input)).parse(|_event| {
                count += 1;
            });
            count
        })
    });
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
