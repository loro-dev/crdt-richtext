#![no_main]
use crdt_range::test_utils::{fuzzing, Action};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|actions: Vec<Action>| { fuzzing(2, actions) });
