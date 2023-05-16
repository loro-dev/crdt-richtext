use std::fs::File;

use crdt_richtext::{
    legacy::RangeMap, legacy::TreeRangeMap, Anchor, AnchorRange, AnchorType, Annotation, Behavior,
    OpID,
};
use criterion::{criterion_group, criterion_main, Criterion};
use pprof::flamegraph::{Direction, Options};
use rand::{Rng, SeedableRng};
use string_cache::DefaultAtom;

struct PProfGuard {
    path: String,
    guard: pprof::ProfilerGuard<'static>,
}

impl PProfGuard {
    #[must_use]
    pub fn new(name: &str) -> Self {
        let guard = pprof::ProfilerGuard::new(100).unwrap();
        Self {
            path: name.to_string(),
            guard,
        }
    }
}

impl Drop for PProfGuard {
    fn drop(&mut self) {
        if let Ok(report) = self.guard.report().build() {
            let file = File::create(self.path.as_str()).unwrap();
            let mut options = Options::default();
            options.direction = Direction::Inverted;
            report.flamegraph_with_options(file, &mut options).unwrap();
        };
    }
}

fn a(n: u64) -> Annotation {
    Annotation {
        id: OpID::new(n, 0),
        range_lamport: (0, OpID::new(n, 0)),
        range: AnchorRange {
            start: Anchor {
                id: Some(OpID::new(n, 0)),
                type_: AnchorType::Before,
            },
            end: Anchor {
                id: Some(OpID::new(n, 0)),
                type_: AnchorType::Before,
            },
        },
        behavior: Behavior::Merge,
        type_: DefaultAtom::from(""),
        value: serde_json::Value::Null,
    }
}

#[cfg(feature = "test")]
pub fn bench(c: &mut Criterion) {
    fuzz(c);
    real(c);
}

fn real(c: &mut Criterion) {
    let mut b = c.benchmark_group("real");
    b.bench_function("annotate to 1000 annotations", |b| {
        let guard = PProfGuard::new("target/annotate_flamegraph.svg");
        b.iter(|| {
            let mut gen = rand::rngs::StdRng::seed_from_u64(0);
            let mut map = TreeRangeMap::new();
            map.insert_directly(0, 10000);
            for i in 0..1000 {
                let start = gen.gen_range(0..10000);
                let end = gen.gen_range(start..10000);
                map.annotate(start, end - start, a(i));
            }
        });
        drop(guard);
    });

    b.bench_function("random inserts 10K", |b| {
        let guard = PProfGuard::new("target/insert_flamegraph.svg");
        b.iter(|| {
            let mut gen = rand::rngs::StdRng::seed_from_u64(0);
            let mut map = TreeRangeMap::new();
            map.insert_directly(0, 10000);
            for i in 0..1000 {
                let start = gen.gen_range(0..10000);
                let end = gen.gen_range(start..10000);
                map.annotate(start, end - start, a(i));
            }
            for _ in 0..10_000 {
                let start = gen.gen_range(0..10000);
                let end = gen.gen_range(start..10000);
                map.insert_directly(start, end - start);
            }
        });
        drop(guard);
    });
}

#[cfg(feature = "test")]
fn fuzz(c: &mut Criterion) {
    use arbitrary::Unstructured;
    use crdt_richtext::legacy::test_utils::{fuzzing, Action};
    let mut b = c.benchmark_group("fuzz");
    b.bench_function("5 actors 1000 actions", |b| {
        let mut data = rand::rngs::StdRng::seed_from_u64(0);
        let mut bytes: Vec<u8> = Vec::new();
        for _ in 0..10000 {
            bytes.push(data.gen());
        }

        let mut u = Unstructured::new(&bytes);
        let actions: [Action; 1000] = u.arbitrary().unwrap();
        let actions = actions.to_vec();

        let guard = PProfGuard::new("target/flamegraph.svg");
        b.iter(|| fuzzing(5, actions.clone()));
        drop(guard);
    });
}

#[cfg(not(feature = "test"))]
pub fn bench(c: &mut Criterion) {}

criterion_group!(benches, bench);

criterion_main!(benches);
