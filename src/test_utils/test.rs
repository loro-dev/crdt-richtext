use std::time::Instant;

use super::*;
use ctor::ctor;

pub fn minify_error<T, F, N>(site_num: u8, actions: Vec<T>, f: F, normalize: N)
where
    F: Fn(u8, &mut [T]),
    N: Fn(u8, &mut [T]) -> Vec<T>,
    T: Clone + Debug,
{
    std::panic::set_hook(Box::new(|_info| {
        // ignore panic output
    }));

    let f_ref: *const _ = &f;
    let f_ref: usize = f_ref as usize;
    let actions_clone = actions.clone();
    let action_ref: usize = (&actions_clone) as *const _ as usize;
    #[allow(clippy::blocks_in_if_conditions)]
    if std::panic::catch_unwind(|| {
        // SAFETY: test
        let f = unsafe { &*(f_ref as *const F) };
        // SAFETY: test
        let actions_ref = unsafe { &mut *(action_ref as *mut Vec<T>) };
        f(site_num, actions_ref);
    })
    .is_ok()
    {
        println!("No Error Found");
        return;
    }

    let mut minified = actions.clone();
    let mut candidates = Vec::new();
    for i in 0..actions.len() {
        let mut new = actions.clone();
        new.remove(i);
        candidates.push(new);
    }

    println!("Minifying...");
    let start = Instant::now();
    while let Some(candidate) = candidates.pop() {
        let f_ref: *const _ = &f;
        let f_ref: usize = f_ref as usize;
        let actions_clone = candidate.clone();
        let action_ref: usize = (&actions_clone) as *const _ as usize;
        #[allow(clippy::blocks_in_if_conditions)]
        if std::panic::catch_unwind(|| {
            // SAFETY: test
            let f = unsafe { &*(f_ref as *const F) };
            // SAFETY: test
            let actions_ref = unsafe { &mut *(action_ref as *mut Vec<T>) };
            f(site_num, actions_ref);
        })
        .is_err()
        {
            for i in 0..candidate.len() {
                let mut new = candidate.clone();
                new.remove(i);
                candidates.push(new);
            }
            if candidate.len() < minified.len() {
                minified = candidate;
                println!("New min len={}", minified.len());
            }
            if candidates.len() > 40 {
                candidates.drain(0..30);
            }
        }
        if start.elapsed().as_secs() > 10 && minified.len() <= 4 {
            break;
        }
        if start.elapsed().as_secs() > 60 {
            break;
        }
    }

    let minified = normalize(site_num, &mut minified);
    println!(
        "Old Length {}, New Length {}",
        actions.len(),
        minified.len()
    );
    dbg!(&minified);
    if actions.len() > minified.len() {
        minify_error(site_num, minified, f, normalize);
    }
}

#[ctor]
fn init_color_backtrace() {
    color_backtrace::install();
}

#[test]
fn test_insert_text_after_bold() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    // **12345**67890
    actor.annotate(0..5, "bold");
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["bold"]), 5), ((vec![]), 5),]));
    // **12345xx**67890
    actor.insert(5, 2);
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["bold"]), 7), ((vec![]), 5),]));
    // **12345xx**6xx7890
    actor.insert(8, 2);
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["bold"]), 7), ((vec![]), 7),]));
}

#[test]
fn test_insert_after_link() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    actor.annotate(0..=4, "link");
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["link"]), 5), ((vec![]), 5),]));
    actor.insert(5, 2);
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["link"]), 5), ((vec![]), 7),]));
    actor.insert(4, 2);
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["link"]), 7), ((vec![]), 7),]));
}

#[test]
fn test_sync() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    actor.annotate(0..=4, "link");
    let mut actor_b = Actor::new(1);
    actor.insert(0, 1);
    actor.merge(&actor_b);
    actor_b.merge(&actor);
    actor.check();
    actor.check_eq(&mut actor_b);
}

#[test]
fn test_delete_annotation() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    actor.annotate(0..5, "bold");
    actor.un_annotate(0..3, "bold");
    let spans = actor.get_annotations(..);
    assert_eq!(
        spans,
        make_spans(&[((vec![]), 3), ((vec!["bold"]), 2), ((vec![]), 5),])
    );
    actor.un_annotate(3..6, "bold");
    assert_eq!(actor.get_annotations(..), make_spans(&[((vec![]), 10),]));
}

#[test]
fn test_delete_text_basic() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    actor.annotate(0..5, "bold");
    actor.delete(0, 2);
    assert_eq!(
        actor.get_annotations(..),
        make_spans(&[((vec!["bold"]), 3), ((vec![]), 5)])
    );
}

#[test]
fn test_delete_text_1() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    //**01234**56789
    actor.annotate(0..3, "bold");
    assert_eq!(
        actor.get_annotations(..),
        make_spans(&[((vec!["bold"]), 3), ((vec![]), 7)])
    );
    actor.insert(2, 2);
    assert_eq!(
        actor.get_annotations(..),
        make_spans(&[((vec!["bold"]), 5), ((vec![]), 7)])
    );
    //**012**6789
    actor.delete(3, 3);
    assert_eq!(
        actor.get_annotations(..),
        make_spans(&[((vec!["bold"]), 3), ((vec![]), 6)])
    );
}

#[test]
fn test_delete_text_then_insert() {
    let mut actor = Actor::new(0);
    let mut b = Actor::new(1);
    actor.insert(0, 10);
    // **ABCDE**FGHIJ
    actor.annotate(0..5, "bold");
    // **ABC**FGHIJ
    actor.delete(3, 2);
    // **ABCxx**FGHIJ
    actor.insert(4, 2);
    b.merge(&actor);
    assert_eq!(
        b.get_annotations(..),
        make_spans(&[(vec!["bold"], 3), (vec![], 7)])
    );
}

#[test]
fn test_patch_expand() {
    let mut a = Actor::new(0);
    let mut b = Actor::new(1);
    let mut c = Actor::new(2);
    a.insert(0, 5);
    b.merge(&a);
    a.delete(2, 2);
    b.annotate(0..=3, "link");
    b.insert(3, 2);
    c.merge(&b);
    c.insert(5, 1);
    a.merge(&b);
    b.merge(&a);
    assert_eq!(a.get_annotations(..), b.get_annotations(..));
    c.merge(&a);
    a.merge(&c);
    assert_eq!(a.get_annotations(..), c.get_annotations(..));
}

#[test]
fn madness() {
    let mut a = Actor::new(0);
    let mut b = Actor::new(1);
    a.insert(0, 5);
    a.annotate(0..2, "bold");
    a.annotate(0..=3, "link");
    a.delete(2, 2);
    a.insert(2, 1);
    assert_eq!(
        a.get_annotations(..),
        make_spans(&[(vec!["bold", "link"], 2), (vec!["bold"], 1), (vec![], 1)])
    );
    b.merge(&a);
    assert_eq!(a.get_annotations(..), b.get_annotations(..));
}

#[test]
fn weird_link() {
    let mut a = Actor::new(0);
    a.insert(0, 10);
    a.annotate(3..=6, "link");
    // 012<3456>789
    assert_eq!(
        a.get_annotations(..),
        make_spans(&[(vec![], 3), (vec!["link"], 4), (vec![], 3)])
    );
    a.delete(3, 3);
    // 012<6>789
    assert_eq!(
        a.get_annotations(..),
        make_spans(&[(vec![], 3), (vec!["link"], 1), (vec![], 3)])
    );
    // 012<6>789
    a.insert(3, 1);
    // 012x<6>789
    assert_eq!(
        a.get_annotations(..),
        make_spans(&[(vec![], 4), (vec!["link"], 1), (vec![], 3)])
    );
    let mut b = Actor::new(1);
    b.merge(&a);
    assert_eq!(a.get_annotations(..), b.get_annotations(..));
}

#[cfg(test)]
mod failed_tests {
    use super::*;
    use Action::*;
    use AnnotationType::*;

    #[test]
    fn fuzz() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 8,
                    annotation: Bold,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 6,
                },
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
            ],
        )
    }

    #[test]
    fn fuzz_1() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 4,
                    annotation: Link,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 6,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
            ],
        )
    }

    #[test]
    fn fuzz_2() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 188,
                    pos: 128,
                    len: 4,
                },
                Annotate {
                    actor: 4,
                    pos: 205,
                    len: 1,
                    annotation: UnLink,
                },
                Sync(255, 255),
                Insert {
                    actor: 0,
                    pos: 16,
                    len: 5,
                },
                Delete {
                    actor: 125,
                    pos: 125,
                    len: 125,
                },
                Insert {
                    actor: 0,
                    pos: 114,
                    len: 57,
                },
            ],
        )
    }

    #[test]
    fn fuzz_3() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Annotate {
                    actor: 0,
                    pos: 1,
                    len: 3,
                    annotation: Link,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: Link,
                },
                Insert {
                    actor: 0,
                    pos: 2,
                    len: 10,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 7,
                    len: 10,
                },
            ],
        )
    }

    #[test]
    fn fuzz_4() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 3,
                },
                Annotate {
                    actor: 98,
                    pos: 0,
                    len: 52,
                    annotation: Bold,
                },
                Sync(251, 255),
                Insert {
                    actor: 57,
                    pos: 57,
                    len: 57,
                },
                Insert {
                    actor: 57,
                    pos: 57,
                    len: 57,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 255,
                },
                Delete {
                    actor: 6,
                    pos: 6,
                    len: 6,
                },
            ],
        )
    }

    #[test]
    fn fuzz_5() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 5,
                },
                Annotate {
                    actor: 0,
                    pos: 1,
                    len: 1,
                    annotation: UnLink,
                },
                Annotate {
                    actor: 0,
                    pos: 2,
                    len: 2,
                    annotation: UnBold,
                },
                Sync(1, 0),
                Insert {
                    actor: 1,
                    pos: 3,
                    len: 2,
                },
                Delete {
                    actor: 0,
                    pos: 2,
                    len: 2,
                },
            ],
        )
    }

    #[test]
    fn fuzz_6() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 4,
                },
                Annotate {
                    actor: 0,
                    pos: 0,
                    len: 1,
                    annotation: UnLink,
                },
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 1,
                    len: 5,
                },
                Delete {
                    actor: 1,
                    pos: 0,
                    len: 2,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
            ],
        )
    }

    #[test]
    fn fuzz_7() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Sync(1, 0),
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: Link,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 9,
                },
                Insert {
                    actor: 1,
                    pos: 3,
                    len: 1,
                },
                Insert {
                    actor: 1,
                    pos: 5,
                    len: 1,
                },
            ],
        )
    }

    #[test]
    fn fuzz_8() {
        fuzzing(
            2,
            vec![
                // 0123456789
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Sync(1, 0),
                // 012<345>6789
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: Link,
                },
                // 012<3>89
                Delete {
                    actor: 0,
                    pos: 4,
                    len: 4,
                },
                // 01234567x89
                Insert {
                    actor: 1,
                    pos: 8,
                    len: 1,
                },
            ],
        )
    }

    #[test]
    fn fuzz_9() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 50,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 212,
                    annotation: Link,
                },
                Sync(255, 26),
                Insert {
                    actor: 65,
                    pos: 190,
                    len: 190,
                },
                Annotate {
                    actor: 255,
                    pos: 190,
                    len: 255,
                    annotation: UnLink,
                },
                Delete {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Annotate {
                    actor: 128,
                    pos: 255,
                    len: 255,
                    annotation: UnLink,
                },
                Delete {
                    actor: 65,
                    pos: 0,
                    len: 0,
                },
                Delete {
                    actor: 75,
                    pos: 65,
                    len: 65,
                },
                Insert {
                    actor: 1,
                    pos: 75,
                    len: 75,
                },
            ],
        )
    }

    #[test]
    fn fuzz_10() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 43,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 200,
                    len: 190,
                    annotation: UnLink,
                },
                Sync(255, 255),
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 158,
                    annotation: UnLink,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: Link,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Sync(11, 10),
                Sync(255, 128),
                Insert {
                    actor: 35,
                    pos: 255,
                    len: 247,
                },
            ],
        )
    }

    #[test]
    fn fuzz_11() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 43,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 200,
                    len: 190,
                    annotation: UnLink,
                },
                Sync(255, 11),
                Annotate {
                    actor: 255,
                    pos: 255,
                    len: 66,
                    annotation: UnBold,
                },
                Sync(255, 247),
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: Link,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
            ],
        )
    }

    #[test]
    fn fuzz_12() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 43,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Sync(207, 207),
                Delete {
                    actor: 65,
                    pos: 255,
                    len: 7,
                },
                Annotate {
                    actor: 48,
                    pos: 193,
                    len: 190,
                    annotation: UnBold,
                },
                Sync(1, 0),
                Annotate {
                    actor: 48,
                    pos: 48,
                    len: 48,
                    annotation: Link,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: Link,
                },
                Insert {
                    actor: 190,
                    pos: 48,
                    len: 190,
                },
            ],
        )
    }

    #[test]
    fn fuzz_13() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Annotate {
                    actor: 0,
                    pos: 4,
                    len: 4,
                    annotation: Bold,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 8,
                },
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    len: 10,
                },
            ],
        )
    }

    #[test]
    fn fuzz_14() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 57,
                    len: 168,
                },
                Delete {
                    actor: 168,
                    pos: 168,
                    len: 168,
                },
                Annotate {
                    actor: 168,
                    pos: 168,
                    len: 168,
                    annotation: UnBold,
                },
                Delete {
                    actor: 112,
                    pos: 112,
                    len: 112,
                },
                Annotate {
                    actor: 168,
                    pos: 255,
                    len: 255,
                    annotation: Link,
                },
                Annotate {
                    actor: 98,
                    pos: 174,
                    len: 0,
                    annotation: UnLink,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 255,
                },
                Annotate {
                    actor: 168,
                    pos: 168,
                    len: 168,
                    annotation: UnBold,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Delete {
                    actor: 112,
                    pos: 112,
                    len: 112,
                },
                Delete {
                    actor: 26,
                    pos: 26,
                    len: 0,
                },
                Insert {
                    actor: 4,
                    pos: 247,
                    len: 255,
                },
            ],
        )
    }

    #[test]
    fn fuzz_15() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: UnBold,
                },
                Sync(1, 0),
                Delete {
                    actor: 1,
                    pos: 2,
                    len: 7,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: UnLink,
                },
                Annotate {
                    actor: 0,
                    pos: 10,
                    len: 0,
                    annotation: Link,
                },
                Delete {
                    actor: 0,
                    pos: 4,
                    len: 4,
                },
                Insert {
                    actor: 0,
                    pos: 4,
                    len: 10,
                },
                Insert {
                    actor: 1,
                    pos: 2,
                    len: 10,
                },
            ],
        )
    }

    #[test]
    fn fuzz_16() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Annotate {
                    actor: 0,
                    pos: 8,
                    len: 0,
                    annotation: Link,
                },
                Annotate {
                    actor: 0,
                    pos: 7,
                    len: 1,
                    annotation: UnBold,
                },
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 4,
                    len: 10,
                },
                Annotate {
                    actor: 1,
                    pos: 3,
                    len: 3,
                    annotation: Bold,
                },
                Delete {
                    actor: 1,
                    pos: 4,
                    len: 4,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 10,
                    annotation: Link,
                },
            ],
        )
    }

    #[test]
    fn fuzz_17() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Sync(1, 0),
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: Link,
                },
                Delete {
                    actor: 0,
                    pos: 4,
                    len: 4,
                },
                Sync(1, 0),
                Insert {
                    actor: 1,
                    pos: 3,
                    len: 10,
                },
                Insert {
                    actor: 1,
                    pos: 14,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 4,
                },
                Delete {
                    actor: 0,
                    pos: 1,
                    len: 0,
                },
                Insert {
                    actor: 1,
                    pos: 12,
                    len: 10,
                },
            ],
        )
    }

    #[test]
    fn fuzz_18() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 1,
                    pos: 0,
                    len: 10,
                },
                Annotate {
                    actor: 1,
                    pos: 1,
                    len: 1,
                    annotation: Bold,
                },
                Sync(0, 1),
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Delete {
                    actor: 1,
                    pos: 2,
                    len: 10,
                },
                Insert {
                    actor: 1,
                    pos: 2,
                    len: 10,
                },
            ],
        )
    }

    #[test]
    fn fuzz_19() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Insert {
                    actor: 190,
                    pos: 43,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnLink,
                },
                Sync(255, 255),
                Annotate {
                    actor: 83,
                    pos: 49,
                    len: 255,
                    annotation: UnLink,
                },
                Annotate {
                    actor: 189,
                    pos: 255,
                    len: 86,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Annotate {
                    actor: 161,
                    pos: 161,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnLink,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Insert {
                    actor: 94,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 189,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnLink,
                },
                Annotate {
                    actor: 190,
                    pos: 66,
                    len: 66,
                    annotation: Bold,
                },
                Annotate {
                    actor: 172,
                    pos: 172,
                    len: 172,
                    annotation: Bold,
                },
                Annotate {
                    actor: 190,
                    pos: 151,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 70,
                    len: 70,
                    annotation: Bold,
                },
                Insert {
                    actor: 11,
                    pos: 247,
                    len: 255,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 114,
                },
                Delete {
                    actor: 255,
                    pos: 247,
                    len: 251,
                },
                Delete {
                    actor: 78,
                    pos: 78,
                    len: 63,
                },
                Annotate {
                    actor: 161,
                    pos: 161,
                    len: 201,
                    annotation: UnLink,
                },
                Insert {
                    actor: 205,
                    pos: 190,
                    len: 190,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Sync(255, 255),
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 188,
                    pos: 11,
                    len: 11,
                },
                Insert {
                    actor: 255,
                    pos: 128,
                    len: 4,
                },
                Insert {
                    actor: 48,
                    pos: 48,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Sync(190, 190),
                Sync(26, 26),
                Insert {
                    actor: 48,
                    pos: 35,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Insert {
                    actor: 247,
                    pos: 255,
                    len: 3,
                },
                Insert {
                    actor: 205,
                    pos: 1,
                    len: 0,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 161,
                    len: 161,
                },
                Sync(255, 255),
                Sync(190, 255),
                Sync(255, 4),
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Insert {
                    actor: 11,
                    pos: 247,
                    len: 255,
                },
                Annotate {
                    actor: 4,
                    pos: 205,
                    len: 1,
                    annotation: Link,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 15,
                    pos: 201,
                    len: 190,
                },
                Sync(247, 26),
                Annotate {
                    actor: 48,
                    pos: 48,
                    len: 35,
                    annotation: Bold,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Sync(255, 255),
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 188,
                    pos: 11,
                    len: 11,
                },
                Insert {
                    actor: 255,
                    pos: 128,
                    len: 4,
                },
                Insert {
                    actor: 40,
                    pos: 48,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
            ],
        )
    }

    #[test]
    fn fuzz_20() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Insert {
                    actor: 0,
                    pos: 10,
                    len: 10,
                },
                Sync(1, 0),
                Annotate {
                    actor: 1,
                    pos: 0,
                    len: 10,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 1,
                    pos: 0,
                    len: 10,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 10,
                    annotation: Bold,
                },
                Delete {
                    actor: 0,
                    pos: 9,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 1,
                    len: 1,
                },
                Sync(0, 1),
                Insert {
                    actor: 0,
                    pos: 8,
                    len: 10,
                },
                Delete {
                    actor: 1,
                    pos: 3,
                    len: 10,
                },
                Insert {
                    actor: 1,
                    pos: 3,
                    len: 1,
                },
            ],
        )
    }

    #[test]
    fn fuzz_21() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Insert {
                    actor: 0,
                    pos: 10,
                    len: 10,
                },
                Sync(1, 0),
                Annotate {
                    actor: 1,
                    pos: 0,
                    len: 10,
                    annotation: Bold,
                },
                Delete {
                    actor: 1,
                    pos: 14,
                    len: 4,
                },
                Insert {
                    actor: 0,
                    pos: 6,
                    len: 10,
                },
                Delete {
                    actor: 1,
                    pos: 0,
                    len: 10,
                },
                Sync(0, 1),
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    len: 1,
                },
            ],
        )
    }

    #[test]
    fn fuzz_22() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Insert {
                    actor: 190,
                    pos: 43,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnLink,
                },
                Sync(255, 255),
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Sync(190, 189),
                Sync(86, 189),
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Insert {
                    actor: 190,
                    pos: 158,
                    len: 190,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 114,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 255,
                },
                Delete {
                    actor: 78,
                    pos: 78,
                    len: 78,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnLink,
                },
                Annotate {
                    actor: 190,
                    pos: 66,
                    len: 66,
                    annotation: Bold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 53,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 156,
                    len: 156,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 172,
                    pos: 172,
                    len: 172,
                    annotation: Bold,
                },
                Annotate {
                    actor: 189,
                    pos: 190,
                    len: 151,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 158,
                    pos: 190,
                    len: 190,
                    annotation: UnLink,
                },
                Sync(78, 78),
                Insert {
                    actor: 64,
                    pos: 161,
                    len: 161,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 255,
                    pos: 255,
                    len: 255,
                    annotation: UnBold,
                },
                Delete {
                    actor: 66,
                    pos: 66,
                    len: 75,
                },
                Annotate {
                    actor: 172,
                    pos: 172,
                    len: 189,
                    annotation: Bold,
                },
                Annotate {
                    actor: 151,
                    pos: 190,
                    len: 0,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnLink,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 114,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 255,
                },
                Delete {
                    actor: 78,
                    pos: 78,
                    len: 78,
                },
                Annotate {
                    actor: 161,
                    pos: 161,
                    len: 161,
                    annotation: UnBold,
                },
                Sync(26, 26),
                Insert {
                    actor: 48,
                    pos: 48,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 65,
                },
                Delete {
                    actor: 255,
                    pos: 255,
                    len: 255,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 114,
                },
                Delete {
                    actor: 255,
                    pos: 247,
                    len: 251,
                },
                Delete {
                    actor: 78,
                    pos: 78,
                    len: 63,
                },
                Annotate {
                    actor: 161,
                    pos: 161,
                    len: 201,
                    annotation: UnLink,
                },
                Insert {
                    actor: 205,
                    pos: 190,
                    len: 190,
                },
                Insert {
                    actor: 11,
                    pos: 247,
                    len: 255,
                },
            ],
        )
    }

    #[test]
    fn fuzz_23() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Insert {
                    actor: 190,
                    pos: 43,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnLink,
                },
                Sync(255, 255),
                Annotate {
                    actor: 83,
                    pos: 49,
                    len: 255,
                    annotation: UnLink,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnLink,
                },
                Annotate {
                    actor: 190,
                    pos: 66,
                    len: 66,
                    annotation: Bold,
                },
                Annotate {
                    actor: 172,
                    pos: 172,
                    len: 172,
                    annotation: Bold,
                },
                Annotate {
                    actor: 190,
                    pos: 151,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: Comment,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 114,
                },
                Annotate {
                    actor: 78,
                    pos: 78,
                    len: 78,
                    annotation: Bold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnLink,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 114,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 255,
                },
                Delete {
                    actor: 78,
                    pos: 78,
                    len: 78,
                },
                Annotate {
                    actor: 161,
                    pos: 161,
                    len: 161,
                    annotation: UnBold,
                },
                Sync(26, 26),
                Insert {
                    actor: 48,
                    pos: 48,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 65,
                },
                Delete {
                    actor: 255,
                    pos: 255,
                    len: 255,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 114,
                },
                Delete {
                    actor: 255,
                    pos: 247,
                    len: 251,
                },
                Delete {
                    actor: 78,
                    pos: 78,
                    len: 63,
                },
                Insert {
                    actor: 205,
                    pos: 190,
                    len: 190,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Sync(255, 255),
            ],
        )
    }

    #[test]
    fn fuzz_24() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Insert {
                    actor: 190,
                    pos: 43,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnLink,
                },
                Sync(255, 255),
                Insert {
                    actor: 0,
                    pos: 41,
                    len: 0,
                },
                Insert {
                    actor: 5,
                    pos: 5,
                    len: 5,
                },
                Delete {
                    actor: 105,
                    pos: 11,
                    len: 10,
                },
                Insert {
                    actor: 5,
                    pos: 155,
                    len: 190,
                },
                Delete {
                    actor: 66,
                    pos: 66,
                    len: 66,
                },
                Insert {
                    actor: 36,
                    pos: 36,
                    len: 36,
                },
                Sync(255, 255),
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 16,
                },
            ],
        )
    }

    #[test]
    fn fuzz_25() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 56,
                    annotation: Link,
                },
                Annotate {
                    actor: 40,
                    pos: 190,
                    len: 190,
                    annotation: Bold,
                },
                Delete {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Sync(57, 57),
                Delete {
                    actor: 65,
                    pos: 65,
                    len: 190,
                },
            ],
        )
    }

    #[test]
    fn fuzz_26() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 43,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Sync(207, 207),
                Delete {
                    actor: 65,
                    pos: 255,
                    len: 7,
                },
                Annotate {
                    actor: 48,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Sync(1, 0),
                Annotate {
                    actor: 48,
                    pos: 255,
                    len: 255,
                    annotation: UnLink,
                },
                Insert {
                    actor: 48,
                    pos: 48,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 244,
                },
                Sync(7, 48),
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 43,
                },
            ],
        )
    }

    #[test]
    fn fuzz_27() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 112,
                    pos: 112,
                    len: 199,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 205,
                },
                Insert {
                    actor: 13,
                    pos: 13,
                    len: 13,
                },
                Insert {
                    actor: 13,
                    pos: 13,
                    len: 13,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 48,
                },
                Insert {
                    actor: 0,
                    pos: 255,
                    len: 255,
                },
                Insert {
                    actor: 57,
                    pos: 51,
                    len: 112,
                },
                Insert {
                    actor: 1,
                    pos: 247,
                    len: 57,
                },
                Delete {
                    actor: 205,
                    pos: 0,
                    len: 255,
                },
                Insert {
                    actor: 41,
                    pos: 0,
                    len: 122,
                },
                Sync(35, 0),
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 43,
                },
                Delete {
                    actor: 103,
                    pos: 103,
                    len: 103,
                },
            ],
        )
    }

    #[test]
    fn fuzz_28() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Insert {
                    actor: 190,
                    pos: 43,
                    len: 190,
                },
                Sync(255, 255),
                Delete {
                    actor: 189,
                    pos: 189,
                    len: 189,
                },
                Delete {
                    actor: 66,
                    pos: 66,
                    len: 75,
                },
                Insert {
                    actor: 52,
                    pos: 56,
                    len: 49,
                },
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 66,
                },
                Delete {
                    actor: 66,
                    pos: 75,
                    len: 189,
                },
                Annotate {
                    actor: 189,
                    pos: 189,
                    len: 189,
                    annotation: Link,
                },
                Insert {
                    actor: 56,
                    pos: 49,
                    len: 57,
                },
                Insert {
                    actor: 48,
                    pos: 54,
                    len: 48,
                },
                Insert {
                    actor: 48,
                    pos: 55,
                    len: 189,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 255,
                },
                Delete {
                    actor: 78,
                    pos: 78,
                    len: 78,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 49,
                    pos: 53,
                    len: 50,
                },
                Insert {
                    actor: 54,
                    pos: 50,
                    len: 48,
                },
                Insert {
                    actor: 190,
                    pos: 158,
                    len: 190,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 114,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 255,
                },
                Delete {
                    actor: 78,
                    pos: 78,
                    len: 78,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Sync(172, 189),
                Delete {
                    actor: 75,
                    pos: 189,
                    len: 189,
                },
                Insert {
                    actor: 189,
                    pos: 189,
                    len: 189,
                },
                Delete {
                    actor: 114,
                    pos: 114,
                    len: 114,
                },
                Delete {
                    actor: 66,
                    pos: 66,
                    len: 66,
                },
                Insert {
                    actor: 39,
                    pos: 189,
                    len: 189,
                },
                Insert {
                    actor: 78,
                    pos: 78,
                    len: 78,
                },
            ],
        )
    }

    #[test]
    fn fuzz_29() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: Bold,
                },
                Sync(1, 0),
                Delete {
                    actor: 1,
                    pos: 2,
                    len: 7,
                },
                Annotate {
                    actor: 0,
                    pos: 2,
                    len: 2,
                    annotation: Link,
                },
                Insert {
                    actor: 0,
                    pos: 4,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 7,
                    len: 10,
                },
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 3,
                    len: 10,
                },
            ],
        )
    }

    #[test]
    fn fuzz_30() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: Link,
                },
                Delete {
                    actor: 0,
                    pos: 3,
                    len: 3,
                },
                Insert {
                    actor: 0,
                    pos: 6,
                    len: 10,
                },
                Insert {
                    actor: 0,
                    pos: 3,
                    len: 10,
                },
            ],
        )
    }

    #[test]
    fn fuzz_31() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Insert {
                    actor: 0,
                    pos: 4,
                    len: 10,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    len: 10,
                },
                Annotate {
                    actor: 0,
                    pos: 1,
                    len: 10,
                    annotation: Bold,
                },
                Insert {
                    actor: 0,
                    pos: 20,
                    len: 10,
                },
                Sync(1, 0),
                Annotate {
                    actor: 1,
                    pos: 0,
                    len: 10,
                    annotation: UnBold,
                },
                Sync(0, 1),
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Delete {
                    actor: 1,
                    pos: 0,
                    len: 10,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    len: 1,
                },
            ],
        )
    }

    #[test]
    fn fuzz_32() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Sync(1, 0),
                Annotate {
                    actor: 1,
                    pos: 5,
                    len: 10,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 1,
                    pos: 5,
                    len: 10,
                    annotation: UnBold,
                },
                Delete {
                    actor: 1,
                    pos: 5,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Insert {
                    actor: 1,
                    pos: 10,
                    len: 10,
                },
                Sync(0, 1),
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 17,
                    len: 10,
                },
                Insert {
                    actor: 0,
                    pos: 25,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Delete {
                    actor: 0,
                    pos: 19,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 7,
                    len: 10,
                },
                Insert {
                    actor: 0,
                    pos: 1,
                    len: 10,
                },
                Insert {
                    actor: 0,
                    pos: 12,
                    len: 10,
                },
            ],
        )
    }

    #[test]
    fn fuzz_33() {
        fuzzing(
            5,
            vec![
                Insert {
                    actor: 0,
                    pos: 122,
                    len: 190,
                },
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Sync(47, 190),
                Insert {
                    actor: 252,
                    pos: 248,
                    len: 59,
                },
                Annotate {
                    actor: 155,
                    pos: 155,
                    len: 155,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 155,
                    pos: 155,
                    len: 155,
                    annotation: UnBold,
                },
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: Bold,
                },
                Delete {
                    actor: 0,
                    pos: 33,
                    len: 33,
                },
                Insert {
                    actor: 0,
                    pos: 50,
                    len: 190,
                },
                Sync(47, 190),
                Delete {
                    actor: 105,
                    pos: 110,
                    len: 107,
                },
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 108,
                },
                Insert {
                    actor: 252,
                    pos: 248,
                    len: 47,
                },
                Insert {
                    actor: 252,
                    pos: 248,
                    len: 59,
                },
                Delete {
                    actor: 177,
                    pos: 185,
                    len: 185,
                },
                Insert {
                    actor: 0,
                    pos: 49,
                    len: 190,
                },
                Delete {
                    actor: 47,
                    pos: 190,
                    len: 190,
                },
                Delete {
                    actor: 70,
                    pos: 0,
                    len: 60,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 49,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Insert {
                    actor: 33,
                    pos: 33,
                    len: 33,
                },
                Sync(211, 211),
                Sync(204, 204),
                Insert {
                    actor: 60,
                    pos: 70,
                    len: 70,
                },
                Insert {
                    actor: 49,
                    pos: 190,
                    len: 47,
                },
                Delete {
                    actor: 70,
                    pos: 0,
                    len: 190,
                },
                Delete {
                    actor: 0,
                    pos: 33,
                    len: 33,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Sync(15, 70),
                Insert {
                    actor: 252,
                    pos: 248,
                    len: 47,
                },
                Delete {
                    actor: 70,
                    pos: 0,
                    len: 60,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 49,
                },
                Delete {
                    actor: 70,
                    pos: 0,
                    len: 60,
                },
                Insert {
                    actor: 252,
                    pos: 248,
                    len: 47,
                },
                Sync(211, 211),
                Sync(0, 60),
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 49,
                },
                Insert {
                    actor: 70,
                    pos: 70,
                    len: 253,
                },
                Delete {
                    actor: 70,
                    pos: 0,
                    len: 60,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 49,
                },
                Insert {
                    actor: 60,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 0,
                    len: 190,
                },
            ],
        )
    }

    #[test]
    fn fuzz_34() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: UnBold,
                },
                Sync(1, 0),
                Delete {
                    actor: 1,
                    pos: 2,
                    len: 7,
                },
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: UnBold,
                },
                Insert {
                    actor: 0,
                    pos: 4,
                    len: 4,
                },
            ],
        )
    }

    #[test]
    fn fuzz_35() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    len: 10,
                },
                Sync(1, 0),
                Annotate {
                    actor: 0,
                    pos: 3,
                    len: 3,
                    annotation: Link,
                },
                Delete {
                    actor: 0,
                    pos: 4,
                    len: 4,
                },
                Insert {
                    actor: 1,
                    pos: 3,
                    len: 3,
                },
                Delete {
                    actor: 0,
                    pos: 3,
                    len: 1,
                },
            ],
        )
    }

    #[test]
    fn fuzz_36() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 128,
                    len: 4,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 59,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 36,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 11,
                    annotation: Link,
                },
                Insert {
                    actor: 11,
                    pos: 11,
                    len: 11,
                },
                Insert {
                    actor: 190,
                    pos: 190,
                    len: 190,
                },
                Sync(247, 26),
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 49,
                    annotation: Link,
                },
                Insert {
                    actor: 11,
                    pos: 11,
                    len: 11,
                },
                Insert {
                    actor: 11,
                    pos: 11,
                    len: 11,
                },
                Insert {
                    actor: 11,
                    pos: 11,
                    len: 11,
                },
                Delete {
                    actor: 205,
                    pos: 1,
                    len: 0,
                },
                Annotate {
                    actor: 48,
                    pos: 48,
                    len: 48,
                    annotation: Bold,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Sync(60, 0),
                Insert {
                    actor: 0,
                    pos: 43,
                    len: 8,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Sync(0, 0),
                Insert {
                    actor: 255,
                    pos: 60,
                    len: 0,
                },
                Sync(60, 0),
                Annotate {
                    actor: 48,
                    pos: 48,
                    len: 48,
                    annotation: Link,
                },
                Insert {
                    actor: 3,
                    pos: 3,
                    len: 3,
                },
                Annotate {
                    actor: 48,
                    pos: 48,
                    len: 48,
                    annotation: Link,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 3,
                },
                Insert {
                    actor: 70,
                    pos: 50,
                    len: 70,
                },
                Sync(247, 26),
                Annotate {
                    actor: 190,
                    pos: 48,
                    len: 48,
                    annotation: Bold,
                },
                Delete {
                    actor: 70,
                    pos: 3,
                    len: 3,
                },
                Insert {
                    actor: 48,
                    pos: 48,
                    len: 48,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Insert {
                    actor: 37,
                    pos: 36,
                    len: 3,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 59,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 36,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: Comment,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: Link,
                },
                Insert {
                    actor: 11,
                    pos: 11,
                    len: 11,
                },
                Insert {
                    actor: 11,
                    pos: 190,
                    len: 190,
                },
                Sync(255, 247),
                Annotate {
                    actor: 190,
                    pos: 190,
                    len: 190,
                    annotation: Link,
                },
                Insert {
                    actor: 11,
                    pos: 11,
                    len: 11,
                },
                Insert {
                    actor: 11,
                    pos: 11,
                    len: 11,
                },
                Insert {
                    actor: 11,
                    pos: 11,
                    len: 11,
                },
                Delete {
                    actor: 70,
                    pos: 205,
                    len: 1,
                },
                Sync(190, 48),
                Insert {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 70,
                },
                Delete {
                    actor: 70,
                    pos: 70,
                    len: 255,
                },
            ],
        )
    }

    #[test]
    fn fuzz_empty() {
        fuzzing(2, vec![])
    }

    #[test]
    fn fuzz_minimize() {
        minify_error(
            5,
            vec![],
            |n, actions| fuzzing(n as usize, actions.to_vec()),
            |_n, actions| actions.to_vec(),
        )
    }
}
