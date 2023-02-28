use criterion::{criterion_group, criterion_main, Criterion};
use rand::{Rng, SeedableRng};

#[cfg(feature = "test")]
pub fn bench(c: &mut Criterion) {
    use std::fs::File;

    use arbitrary::Unstructured;
    use crdt_range::test_utils::{fuzzing, Action};
    use pprof::flamegraph::{Direction, Options};
    let mut data = rand::rngs::StdRng::seed_from_u64(0);
    let mut bytes: Vec<u8> = Vec::new();
    for _ in 0..10000 {
        bytes.push(data.gen());
    }

    let mut u = Unstructured::new(&bytes);
    let actions: [Action; 1000] = u.arbitrary().unwrap();
    let actions = actions.to_vec();
    let mut b = c.benchmark_group("fuzz");
    let guard = pprof::ProfilerGuard::new(100).unwrap();
    b.bench_function("5 actors 1000 actions", |b| {
        b.iter(|| fuzzing(5, actions.clone()));
    });
    if let Ok(report) = guard.report().build() {
        let file = File::create("target/flamegraph.svg").unwrap();
        let mut options = Options::default();
        options.direction = Direction::Inverted;
        report.flamegraph_with_options(file, &mut options).unwrap();
    };
}

#[cfg(not(feature = "test"))]
pub fn bench(c: &mut Criterion) {}

criterion_group!(benches, bench);

criterion_main!(benches);
