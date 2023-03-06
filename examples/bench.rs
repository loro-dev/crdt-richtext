use crdt_range::{Anchor, AnchorRange, AnchorType, Annotation, OpID, RangeMergeRule};
// #[global_allocator]
// static ALLOC: dhat::Alloc = dhat::Alloc;

use crdt_range::{RangeMap, TreeRangeMap};
use rand::{Rng, SeedableRng};
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
        merge_method: RangeMergeRule::Merge,
        type_: String::new(),
        meta: None,
    }
}

pub fn main() {
    for _ in 0..1 {
        let mut gen = rand::rngs::StdRng::seed_from_u64(0);
        let mut map = TreeRangeMap::new();
        // let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
        map.insert_directly(0, 10000);
        for i in 0..10_000 {
            let start = gen.gen_range(0..10000);
            let end = gen.gen_range(start..10000);
            map.annotate(start, end - start, a(i));
        }
        for _ in 0..100_000 {
            let start = gen.gen_range(0..10000);
            let end = gen.gen_range(start..10000);
            map.insert_directly(start, end - start);
        }
        for _ in 0..10_000 {
            let start = gen.gen_range(0..map.len());
            let end = gen.gen_range(start..map.len());
            map.delete(start, (end - start).min(20));
        }
        // drop(profiler);
        // dbg!(&map);
    }
}
