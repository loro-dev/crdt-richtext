use super::*;
use ctor::ctor;

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
    actor.check_eq(&actor_b);
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
    fn fuzz_empty() {
        fuzzing(2, vec![])
    }
}
