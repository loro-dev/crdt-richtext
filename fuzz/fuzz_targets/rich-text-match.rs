#![no_main]
use crdt_richtext::rich_text::test_utils::{fuzzing_line_break, LineBreakFuzzAction};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|actions: Vec<LineBreakFuzzAction>| { fuzzing_line_break(actions) });
