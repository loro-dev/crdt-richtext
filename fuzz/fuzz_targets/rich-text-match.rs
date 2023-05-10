#![no_main]
use crdt_richtext::rich_text::test_utils::{fuzzing_match_str, Action};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|actions: Vec<Action>| { fuzzing_match_str(actions) });
