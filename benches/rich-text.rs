use automerge::get_automerge_actions;
use crdt_richtext::rich_text::RichText;
use criterion::{criterion_group, criterion_main, Criterion};
mod automerge;

pub fn bench(c: &mut Criterion) {
    c.bench_function("automerge", |b| {
        let actions = get_automerge_actions();
        b.iter(|| {
            let mut text = RichText::new(1);
            for action in actions.iter() {
                if action.del > 0 {
                    text.delete(action.pos..action.pos + action.del);
                }
                if !action.ins.is_empty() {
                    text.insert(action.pos, &action.ins);
                }
            }
        })
    });

    c.bench_function("automerge apply", |bench| {
        let actions = get_automerge_actions();
        let mut a = RichText::new(1);
        for action in actions.iter() {
            if action.del > 0 {
                a.delete(action.pos..action.pos + action.del);
            }
            if !action.ins.is_empty() {
                a.insert(action.pos, &action.ins);
            }
        }

        bench.iter(|| {
            let mut b = RichText::new(1);
            b.merge(&a);
        });
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
