#![no_main]
use crdt_richtext::rich_text::test_utils::{fuzzing_utf16, Action};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|actions: Vec<Action>| { fuzzing_utf16(5, actions) });
