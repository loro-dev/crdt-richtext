#[cfg(feature = "test")]
pub fn main() {
    use arbitrary::{Arbitrary, Unstructured};
    use crdt_range::test_utils::{fuzzing, Action, AnnotationType};
    use rand::{Rng, SeedableRng};
    use Action::*;
    use AnnotationType::*;
    let mut data = rand::rngs::StdRng::seed_from_u64(0);
    let mut bytes: Vec<u8> = Vec::new();
    for _ in 0..10000 {
        bytes.push(data.gen());
    }

    let mut u = Unstructured::new(&bytes);
    let actions: [Action; 1000] = u.arbitrary().unwrap();
    println!("actions: {:?}", actions.len());
    fuzzing(5, actions.to_vec());
}

#[cfg(not(feature = "test"))]
fn main() {}
