use std::env;

use arbitrary::{Arbitrary, Unstructured};
use crdt_richtext::rich_text::RichText;
use generic_btree::HeapVec;
use rand::{Rng, SeedableRng};

#[derive(Arbitrary, Debug, Clone, Copy)]
enum RandomAction {
    Insert { pos: u8, content: u8 },
    Delete { pos: u8, len: u8 },
}

use std::io::Read;

use flate2::read::GzDecoder;
use serde_json::Value;

#[derive(Arbitrary)]
pub struct TextAction {
    pub pos: usize,
    pub ins: String,
    pub del: usize,
}

pub fn get_automerge_actions() -> Vec<TextAction> {
    const RAW_DATA: &[u8; 901823] = include_bytes!("../benches/automerge-paper.json.gz");
    let mut actions = Vec::new();
    let mut d = GzDecoder::new(&RAW_DATA[..]);
    let mut s = String::new();
    d.read_to_string(&mut s).unwrap();
    let json: Value = serde_json::from_str(&s).unwrap();
    let txns = json.as_object().unwrap().get("txns");
    for txn in txns.unwrap().as_array().unwrap() {
        let patches = txn
            .as_object()
            .unwrap()
            .get("patches")
            .unwrap()
            .as_array()
            .unwrap();
        for patch in patches {
            let pos = patch[0].as_u64().unwrap() as usize;
            let del_here = patch[1].as_u64().unwrap() as usize;
            let ins_content = patch[2].as_str().unwrap();
            actions.push(TextAction {
                pos,
                ins: ins_content.to_string(),
                del: del_here,
            });
        }
    }
    actions
}

pub fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 && args[1].eq_ignore_ascii_case("automerge") {
        println!("Running on automerge dataset");
        let actions = get_automerge_actions();
        bench(actions);
    } else {
        println!("Running on random generated actions 10k");
        let mut rng = rand::rngs::StdRng::seed_from_u64(123);
        let data: HeapVec<u8> = (0..1_000_000).map(|_| rng.gen()).collect();
        let mut gen = Unstructured::new(&data);
        let actions: [RandomAction; 10_000] = gen.arbitrary().unwrap();

        let mut rope = RichText::new(1);
        for _ in 0..10000 {
            for action in actions.iter() {
                match *action {
                    RandomAction::Insert { pos, content } => {
                        let pos = pos as usize % (rope.len() + 1);
                        let s = content.to_string();
                        rope.insert(pos, &s);
                    }
                    RandomAction::Delete { pos, len } => {
                        let pos = pos as usize % (rope.len() + 1);
                        let mut len = len as usize % 10;
                        len = len.min(rope.len() - pos);
                        rope.delete(pos..(pos + len));
                    }
                }
            }
        }
    }
}

#[inline(never)]
fn bench(actions: Vec<TextAction>) {
    // #[global_allocator]
    // static ALLOC: dhat::Alloc = dhat::Alloc;
    for _ in 0..10 {
        let mut text = RichText::new(1);
        // let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
        for action in actions.iter() {
            if action.del > 0 {
                text.delete(action.pos..action.pos + action.del);
            }
            if !action.ins.is_empty() {
                text.insert(action.pos, &action.ins)
            }
        }
        // drop(profiler);
        text.debug_log(false)
    }
}
