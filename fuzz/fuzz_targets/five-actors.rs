#![no_main]
use crdt_richtext::legacy::test_utils::{fuzzing, Action};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|actions: [Action; 100]| { fuzzing(5, actions.to_vec()) });
