use criterion::{criterion_group, criterion_main, Criterion};
use rand::{Rng, SeedableRng};

#[cfg(feature = "test")]
pub fn bench(c: &mut Criterion) {
    use arbitrary::Unstructured;
    use crdt_range::test_utils::{fuzzing, Action};
    let mut data = rand::rngs::StdRng::seed_from_u64(0);
    let mut bytes: Vec<u8> = Vec::new();
    for _ in 0..10000 {
        bytes.push(data.gen());
    }

    let mut u = Unstructured::new(&bytes);
    let actions: [Action; 1000] = u.arbitrary().unwrap();
    println!("actions: {:?}", actions.len());
    let actions = actions.to_vec();
    c.bench_function("fuzzing 5", |b| {
        b.iter(|| fuzzing(5, actions.clone()));
    });
}

#[cfg(not(feature = "test"))]
pub fn bench(c: &mut Criterion) {}

criterion_group!(benches, bench);
criterion_main!(benches);
