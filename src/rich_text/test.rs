use crate::InternalString;

use super::*;

mod delete {
    use super::*;

    #[test]
    fn delete() {
        let mut text = RichText::new(1);
        text.insert(0, "123");
        text.delete(..1);
        assert_eq!(text.len(), 2);
        assert_eq!(text.to_string().as_str(), "23");
    }

    #[test]
    fn delete_middle() {
        let mut text = RichText::new(1);
        text.insert(0, "123");
        text.delete(1..2);
        assert_eq!(text.len(), 2);
        assert_eq!(text.to_string().as_str(), "13");
    }

    #[test]
    fn delete_end() {
        let mut text = RichText::new(1);
        text.insert(0, "123");
        text.delete(2..);
        assert_eq!(text.len(), 2);
        assert_eq!(text.to_string().as_str(), "12");
    }

    #[test]
    fn delete_all() {
        let mut text = RichText::new(1);
        assert!(text.is_empty());
        text.insert(0, "123");
        assert!(!text.is_empty());
        text.delete(..);
        assert_eq!(text.len(), 0);
        assert!(text.is_empty());
        assert_eq!(text.to_string().as_str(), "");
    }

    #[test]
    fn delete_across_leaf() {
        let mut text = RichText::new(1);
        let mut s = String::new();
        for i in 0..1000 {
            let t = i.to_string();
            text.insert(0, t.as_str());
            s.insert_str(0, &t);
        }

        text.delete(50..300);
        s.drain(50..300);
        assert_eq!(text.to_string(), s);
        text.delete(50..300);
        s.drain(50..300);
        assert_eq!(text.to_string(), s);
        text.delete(0..300);
        s.drain(0..300);
        assert_eq!(text.to_string(), s);
        text.delete(..);
        assert_eq!(text.to_string().as_str(), "");
        assert!(text.is_empty());
        text.check_no_mergeable_neighbor();
    }

    #[test]
    fn delete_should_be_merged() {
        let mut text = RichText::new(1);
        text.insert(0, "12345");
        text.delete(3..4);
        text.delete(1..2);
        text.delete(..);
        let node = text.content.get_node(text.content.first_leaf());
        assert_eq!(node.elements().len(), 1);
    }

    #[test]
    fn delete_should_be_merged_1() {
        let mut text = RichText::new(1);
        text.insert(0, "12345");
        text.delete(4..5);
        text.delete(3..4);
        text.delete(..2);
        let node = text.content.get_node(text.content.first_leaf());
        assert_eq!(node.elements().len(), 3);
    }

    #[test]
    fn delete_op_merge() {
        let mut text = RichText::new(1);
        text.insert(0, "12345");
        text.delete(0..1);
        text.delete(0..1);
        text.delete(0..1);
        assert_eq!(text.store.op_len(), 2);
    }
}

mod insert {
    use super::*;

    #[test]
    fn insert_len() {
        let mut text = RichText::new(1);
        text.insert(0, "123");
        assert!(text.len() == 3);
        assert!(text.utf16_len() == 3);
        text.insert(0, "的");
        assert!(text.len() == 6);
        assert!(text.utf16_len() == 4);
        assert_eq!(text.to_string().as_str(), "的123");
        text.insert(5, "k");
        assert_eq!(text.to_string().as_str(), "的12k3");
    }

    #[test]
    fn utf_16() {
        let mut text = RichText::new(1);
        // insert
        text.insert_utf16(0, "你");
        assert_eq!(text.utf16_len(), 1);
        text.insert_utf16(1, "好");
        assert_eq!(text.utf16_len(), 2);
        assert_eq!(&text.to_string(), "你好");

        // annotate
        text.annotate_utf16(0..1, bold());
        let spans = text.get_spans();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "你");
        text.insert_utf16(1, "k");
        let spans = text.get_spans();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "你k");
        assert_eq!(spans[0].annotations.iter().next().unwrap(), "bold");

        // delete
        text.delete_utf16(0..2);
        assert_eq!(&text.to_string(), "好");
    }

    #[test]
    fn insert_should_merge() {
        let mut text = RichText::new(1);
        for i in 0..10000 {
            text.insert(i, "1")
        }
        assert!(text.content.node_len() < 10);
        assert_eq!(text.utf16_len(), 10000);
        assert_eq!(text.len(), 10000);
    }

    #[test]
    fn merge_insert() {
        let mut text = RichText::new(1);
        text.insert(0, "123");
        let mut b = RichText::new(2);
        b.merge(&text);
        assert_eq!(b.to_string().as_str(), "123");
        text.insert(1, "k");
        b.merge(&text);
        assert_eq!(b.to_string().as_str(), "1k23");
        text.insert(1, "y");
        b.merge(&text);
        assert_eq!(b.to_string().as_str(), "1yk23");

        text.insert(5, "z");
        b.merge(&text);
        assert_eq!(b.to_string().as_str(), "1yk23z");

        for i in 0..100 {
            text.insert(i, i.to_string().as_str());
            b.merge(&text);
            assert_eq!(b.to_string(), text.to_string());
        }

        for i in (0..100).step_by(3) {
            text.insert(i, i.to_string().as_str());
            b.merge(&text);
            assert_eq!(b.to_string(), text.to_string());
        }
    }
}

mod apply {
    use super::*;

    #[test]
    fn apply_delete() {
        let mut a = RichText::new(1);
        let mut b = RichText::new(2);
        a.insert(0, "123");
        a.delete(..1);
        b.merge(&a);
        assert_eq!(a.to_string().as_str(), "23");
        assert_eq!(b.to_string().as_str(), "23");
        a.insert(0, "xyz");
        b.merge(&a);
        assert_eq!(b.to_string().as_str(), "xyz23");
        a.delete(2..4);
        b.merge(&a);
        assert_eq!(b.to_string().as_str(), "xy3");
        a.delete(..);
        b.merge(&a);
        assert!(b.is_empty());
        assert_eq!(b.to_string().as_str(), "");
    }

    #[test]
    fn apply_basic() {
        let mut a = RichText::new(1);
        let mut b = RichText::new(2);
        a.insert(0, "6");
        b.merge(&a);
        b.insert(0, "3");
        a.insert(0, "2");
        a.merge(&b);
        b.merge(&a);
        assert_eq!(a.to_string(), b.to_string());
    }

    #[test]
    fn apply_annotation() {
        let mut a = RichText::new(1);
        let mut b = RichText::new(2);
        a.insert(0, "aaa");
        b.insert(0, "bbb");
        a.annotate(.., bold());
        b.annotate(.., link());
        a.merge(&b);
        b.merge(&a);
        assert_eq!(a.get_spans(), b.get_spans());
    }
}

fn bold() -> Style {
    Style {
        start_type: AnchorType::Before,
        end_type: AnchorType::Before,
        behavior: crate::Behavior::Merge,
        type_: InternalString::from("bold"),
    }
}

fn unbold() -> Style {
    Style {
        start_type: AnchorType::Before,
        end_type: AnchorType::Before,
        behavior: crate::Behavior::Delete,
        type_: InternalString::from("bold"),
    }
}

fn link() -> Style {
    Style {
        start_type: AnchorType::Before,
        end_type: AnchorType::After,
        behavior: crate::Behavior::Merge,
        type_: InternalString::from("link"),
    }
}

fn unlink() -> Style {
    Style {
        start_type: AnchorType::After,
        end_type: AnchorType::Before,
        behavior: crate::Behavior::Delete,
        type_: InternalString::from("link"),
    }
}

fn expanding_style() -> Style {
    Style {
        start_type: AnchorType::After,
        end_type: AnchorType::Before,
        behavior: crate::Behavior::Merge,
        type_: InternalString::from("expand"),
    }
}

mod annotation {

    use super::*;

    #[test]
    fn annotate_bold() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(0..=2, bold());
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans.len(), 2);
        assert_eq!(ans[0].len(), 3);
        assert_eq!(ans[1].len(), 6);
        assert_eq!(ans[0].as_str(), "123");
    }

    #[test]
    fn annotate_link() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(0..3, link());
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans.len(), 2);
        assert_eq!(ans[0].len(), 3);
        assert_eq!(ans[1].len(), 6);
        assert_eq!(ans[0].as_str(), "123");
        assert!(ans[0].annotations.contains(&"link".into()));
    }

    #[test]
    fn annotate_link_single_char() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(3..=3, link());
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans.len(), 3);
        assert_eq!(ans[0].len(), 3);
        assert_eq!(ans[1].len(), 1);
        assert_eq!(ans[2].len(), 5);
        assert_eq!(ans[1].as_str(), "4");
        assert!(ans[1].annotations.contains(&"link".into()));
    }

    #[test]
    fn annotate_whole_doc() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(.., expanding_style());
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans.len(), 1);
        assert_eq!(ans[0].len(), 9);
        assert_eq!(ans[0].as_str(), "123456789");
        text.delete(..);
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans.len(), 0);
        text.insert(0, "123456789");
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans.len(), 1);
        assert_eq!(ans[0].len(), 9);
        assert_eq!(ans[0].as_str(), "123456789");
        assert!(ans[0].annotations.contains(&"expand".into()));
    }

    #[test]
    fn annotate_half_doc_start() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(..5, expanding_style());
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans.len(), 2);
        assert_eq!(ans[0].len(), 5);
        assert_eq!(ans[1].len(), 4);
        assert!(ans[0].annotations.contains(&"expand".into()));

        // should expand
        text.insert(5, "k");
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans[0].len(), 6);
        assert!(ans[0].annotations.contains(&"expand".into()));

        text.delete(3..7);
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans[0].len(), 3);
        assert!(ans[0].annotations.contains(&"expand".into()));

        text.insert(3, "k");
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans[0].len(), 4);

        text.delete(0..5);
        text.insert(0, "12");
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans[0].len(), 2);
        assert!(ans[0].annotations.contains(&"expand".into()));
    }

    #[test]
    fn annotate_half_doc_end() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(5.., expanding_style());
        {
            let ans = text.iter().collect::<Vec<_>>();
            assert_eq!(ans.len(), 2);
            assert_eq!(ans[0].len(), 5);
            assert_eq!(ans[0].annotations.len(), 0);
            assert_eq!(ans[1].len(), 4);
            assert_eq!(ans[1].annotations.len(), 1);
        }
        text.delete(4..6);
        {
            let ans = text.iter().collect::<Vec<_>>();
            assert_eq!(ans.len(), 2);
            assert_eq!(ans[0].len(), 4);
            assert_eq!(ans[0].annotations.len(), 0);
            assert_eq!(ans[1].len(), 3);
            assert_eq!(ans[1].annotations.len(), 1);
        }
        text.insert(7, "k");
        {
            let ans = text.iter().collect::<Vec<_>>();
            assert_eq!(ans.len(), 2);
            assert_eq!(ans[0].len(), 4);
            assert_eq!(ans[0].annotations.len(), 0);
            assert_eq!(ans[1].as_str(), "789k");
            assert_eq!(ans[1].annotations.len(), 1);
        }
    }

    #[test]
    fn test_simple_unbold() {
        let mut text = RichText::new(1);
        text.insert(0, "123");
        text.annotate(0..1, bold());
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans.len(), 2);
        assert_eq!(ans[0].annotations.len(), 1);

        text.annotate(0..1, unbold());
        let ans = text.iter().collect::<Vec<_>>();
        assert_eq!(ans.len(), 1);
        assert_eq!(ans[0].annotations.len(), 0);
        assert_eq!(&ans[0].text, "123");
    }

    #[test]
    fn test_unbold() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(0..5, bold());
        text.annotate(3..5, unbold());
        {
            let ans = text.iter().collect::<Vec<_>>();
            assert_eq!(ans.len(), 2);
            assert_eq!(ans[0].len(), 3);
            assert_eq!(ans[0].annotations.len(), 1);
            assert_eq!(ans[1].len(), 6);
            assert_eq!(ans[1].annotations.len(), 0);
        }
        text.insert(3, "k");
        {
            let ans = text.iter().collect::<Vec<_>>();
            assert_eq!(ans.len(), 2);
            assert_eq!(ans[0].as_str(), "123k");
            assert_eq!(ans[0].annotations.len(), 1);
            assert_eq!(ans[1].len(), 6);
            assert_eq!(ans[1].annotations.len(), 0);
        }
    }

    #[test]
    fn test_unlink() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(0..5, link());
        text.annotate(3..5, unlink());
        {
            let ans = text.iter().collect::<Vec<_>>();
            assert_eq!(ans.len(), 2);
            assert_eq!(ans[0].len(), 3);
            assert_eq!(ans[0].annotations.len(), 1);
            assert_eq!(ans[1].len(), 6);
            assert_eq!(ans[1].annotations.len(), 0);
        }
        text.insert(3, "k");
        {
            let ans = text.iter().collect::<Vec<_>>();
            assert_eq!(ans.len(), 2);
            assert_eq!(ans[0].as_str(), "123");
            assert_eq!(ans[0].annotations.len(), 1);
            assert_eq!(ans[1].len(), 7);
            assert_eq!(ans[1].annotations.len(), 0);
        }
    }

    #[test]
    fn expand() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(0..5, link());
        text.annotate(0..5, bold());
        {
            let ans = text.get_spans();
            assert_eq!(ans.len(), 2);
            assert_eq!(ans[0].len(), 5);
            assert_eq!(ans[1].len(), 4);
            assert_eq!(ans[0].annotations.len(), 2);
        }
        text.insert(5, "k");
        {
            let ans = text.get_spans();
            assert_eq!(ans.len(), 3);
            assert_eq!(ans[0].len(), 5);
            assert_eq!(ans[1].len(), 1);
            assert_eq!(ans[2].len(), 4);
            assert!(ans[0].annotations.contains(&"link".into()));
            assert!(ans[0].annotations.contains(&"bold".into()));
            assert!(ans[1].annotations.contains(&"bold".into()));
            assert!(ans[2].annotations.is_empty());
        }
    }

    #[test]
    fn shrink() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(0..5, link());
        text.annotate(0..5, bold());
        text.delete(3..7);
        {
            let ans = text.get_spans();
            assert_eq!(ans.len(), 2);
            assert_eq!(ans[0].len(), 3);
            assert_eq!(ans[1].len(), 2);
            assert!(ans[0].annotations.contains(&"link".into()));
            assert!(ans[0].annotations.contains(&"bold".into()));
            assert!(ans[1].annotations.is_empty());
        }
    }

    #[test]
    fn insert_before_tombstone_bold() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(0..5, bold());
        text.delete(4..6);
        text.insert(4, "k");
        let spans = text.get_spans();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "1234k");
        assert!(spans[0].annotations.contains(&"bold".into()));
    }

    #[test]
    fn insert_after_tombstone_link() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(0..5, link());
        text.delete(4..6);
        text.insert(4, "k");
        let spans = text.get_spans();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "1234");
        assert!(spans[0].annotations.contains(&"link".into()));
        assert!(spans[1].annotations.is_empty());
    }

    #[test]
    fn insert_before_bold_anchor_but_after_link_anchor_in_tombstones() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        // end anchor attached to `5`
        text.annotate(0..5, link());
        // end anchor attached to `6`
        text.annotate(0..5, bold());
        // delete `5` and `6`
        text.delete(4..6);
        text.insert(4, "k");
        assert_eq!(text.to_string().as_str(), "1234k789");
        let spans = text.get_spans();
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "1234");
        assert!(spans[0].annotations.contains(&"link".into()));
        assert!(spans[0].annotations.contains(&"bold".into()));
        assert_eq!(spans[1].text, "k");
        assert!(!spans[1].annotations.contains(&"link".into()));
        assert!(spans[1].annotations.contains(&"bold".into()));
        assert_eq!(spans[2].text, "789");
        assert!(spans[2].annotations.is_empty());
    }

    #[test]
    fn apply_remote_annotation() {
        let mut text = RichText::new(1);
        text.insert(0, "123456789");
        text.annotate(0..5, link());
        let mut b = RichText::new(2);
        b.merge(&text);
        assert_eq!(b.get_spans(), text.get_spans());
    }
}

mod fugue {
    use super::*;

    #[test]
    fn test_find_right() {
        let mut text = RichText::new(1);
        text.insert(0, "0");
        text.insert(0, "1");
        let span = text.content.iter().next().unwrap();
        assert_eq!(span.right.unwrap().counter, 0);
        text.insert(0, "2");
        let span = text.content.iter().next().unwrap();
        assert_eq!(span.right.unwrap().counter, 1);
        // before: 210
        text.insert(2, "3");
        // after: 2130
        let span = text.content.iter().nth(2).unwrap();
        assert_eq!(span.right.unwrap().counter, 0);
        let mut other = RichText::new(2);
        other.merge(&text);
        // before: 2130
        other.insert(0, "4");
        // before: 42130
        let span = other.content.iter().next().unwrap();
        assert_eq!(span.right.unwrap().counter, 2);
    }

    #[test]
    fn test_merge_split_right() {
        let mut text = RichText::new(1);
        text.insert(0, "0");
        text.insert(1, "1");
        let span = text.content.iter().next().unwrap();
        assert_eq!(span.rle_len(), 2);
        assert!(span.right.is_none());
        text.insert(1, "2");
        let span = text.content.iter().next().unwrap();
        assert_eq!(span.rle_len(), 1);
        assert!(span.right.is_none());
        let span = text.content.iter().nth(1).unwrap();
        assert_eq!(span.rle_len(), 1);
        assert_eq!(span.right.unwrap().counter, 1);
    }

    #[test]
    fn test_backward_interleaving() {
        let mut a = RichText::new(1);
        a.insert(0, " ");
        a.insert(0, "i");
        a.insert(0, "H");
        let mut b = RichText::new(2);
        b.insert(0, "o");
        a.merge(&b);
        b.insert(0, "l");
        a.merge(&b);
        b.insert(0, "l");
        a.merge(&b);
        b.insert(0, "e");
        a.merge(&b);
        b.insert(0, "H");
        a.merge(&b);
        assert_eq!(&a.to_string(), "Hi Hello");
    }

    #[test]
    fn test_forward_interleaving() {
        let mut a = RichText::new(1);
        a.insert(0, "H");
        a.insert(1, "i");
        a.insert(2, " ");
        let mut b = RichText::new(2);
        b.insert(0, "H");
        b.insert(1, "e");
        b.insert(2, "l");
        b.insert(3, "l");
        b.insert(4, "o");
        a.merge(&b);
        assert_eq!(&a.to_string(), "Hi Hello");
    }
}

mod failed_fuzzing_tests {
    use crate::{
        legacy::test::minify_error,
        rich_text::test_utils::{fuzzing, fuzzing_match_str, Action},
        test_utils::AnnotationType,
    };

    use Action::*;
    use AnnotationType::*;

    #[test]
    fn fuzz_basic() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 72,
                    pos: 72,
                    content: 24648,
                },
                Annotate {
                    actor: 254,
                    pos: 122,
                    len: 133,
                    annotation: Link,
                },
            ],
        )
    }

    #[test]
    fn fuzz_0() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Insert {
                    actor: 0,
                    pos: 129,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 18432,
                },
                Insert {
                    actor: 72,
                    pos: 72,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
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
                    content: 0,
                },
                Insert {
                    actor: 72,
                    pos: 72,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
            ],
        );
    }

    #[test]
    fn fuzz_2() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 18504,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 2,
                },
                Delete {
                    actor: 0,
                    pos: 2,
                    len: 2,
                },
            ],
        );
    }

    #[test]
    fn fuzz_3() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 5395,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 47394,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
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
            ],
        );
    }

    #[test]
    fn fuzz_4() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 32,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 8224,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 8224,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 8224,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 8224,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 8224,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 18464,
                },
                Insert {
                    actor: 0,
                    pos: 72,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 255,
                    len: 255,
                },
            ],
        );
    }

    #[test]
    fn fuzz_5() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 1,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 1,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 3,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 8,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 4,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 21,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 21,
                    content: 65301,
                },
                Sync(1, 0),
                Sync(1, 0),
                Sync(0, 1),
                Delete {
                    actor: 0,
                    pos: 4,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 4,
                    len: 10,
                },
            ],
        );
    }

    #[test]
    fn fuzz_6() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 18504,
                },
                Insert {
                    actor: 20,
                    pos: 20,
                    content: 5140,
                },
                Insert {
                    actor: 20,
                    pos: 20,
                    content: 5140,
                },
                Insert {
                    actor: 20,
                    pos: 20,
                    content: 5140,
                },
                Insert {
                    actor: 20,
                    pos: 20,
                    content: 5140,
                },
                Insert {
                    actor: 20,
                    pos: 255,
                    content: 65535,
                },
                Sync(255, 255),
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Sync(255, 255),
                Delete {
                    actor: 255,
                    pos: 255,
                    len: 255,
                },
            ],
        );
    }

    #[test]
    fn fuzz_7() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 512,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 1,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 1,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 7,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 3,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Sync(1, 0),
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 31,
                    content: 65535,
                },
                Sync(1, 0),
                Sync(1, 0),
                Sync(1, 0),
                Sync(1, 0),
                Delete {
                    actor: 0,
                    pos: 11,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 21,
                    len: 10,
                },
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Sync(0, 1),
                Sync(1, 0),
                Sync(1, 0),
                Delete {
                    actor: 0,
                    pos: 30,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 8,
                    len: 10,
                },
                Delete {
                    actor: 1,
                    pos: 3,
                    len: 10,
                },
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 65535,
                },
            ],
        );
    }

    #[test]
    fn fuzz_8() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 179,
                    pos: 72,
                    len: 21,
                },
                Delete {
                    actor: 29,
                    pos: 29,
                    len: 29,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 10786,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 92,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8994,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 7458,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 156,
                    content: 40092,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8739,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 0,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 2304,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 63228,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 42,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8739,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Sync(231, 231),
                Sync(231, 231),
                Sync(231, 231),
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 42,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 0,
                    content: 0,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 255,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 42,
                },
                Sync(255, 255),
                Insert {
                    actor: 5,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 16674,
                },
                Insert {
                    actor: 0,
                    pos: 4,
                    content: 512,
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
                    actor: 40,
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
                Delete {
                    actor: 34,
                    pos: 34,
                    len: 34,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 22873,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
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
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 22562,
                },
                Delete {
                    actor: 88,
                    pos: 88,
                    len: 88,
                },
                Delete {
                    actor: 88,
                    pos: 88,
                    len: 88,
                },
                Delete {
                    actor: 88,
                    pos: 88,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 34,
                    content: 2338,
                },
                Insert {
                    actor: 9,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 2,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 4,
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
                    actor: 1,
                    pos: 254,
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
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 88,
                    pos: 88,
                    len: 88,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 86,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 34,
                    pos: 65,
                    content: 34,
                },
                Insert {
                    actor: 0,
                    pos: 2,
                    content: 18432,
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
                    actor: 40,
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
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
            ],
        );
    }

    #[test]
    fn fuzz_9() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 184,
                    pos: 183,
                    len: 183,
                },
                Insert {
                    actor: 2,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 255,
                    content: 65535,
                },
                Sync(255, 255),
                Delete {
                    actor: 255,
                    pos: 255,
                    len: 255,
                },
            ],
        );
    }

    #[test]
    fn fuzz_10() {
        fuzzing(
            2,
            vec![
                Annotate {
                    actor: 72,
                    pos: 72,
                    len: 72,
                    annotation: Bold,
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
                Sync(255, 255),
                Sync(255, 255),
                Sync(255, 26),
                Insert {
                    actor: 26,
                    pos: 26,
                    content: 6682,
                },
                Insert {
                    actor: 26,
                    pos: 128,
                    content: 18504,
                },
                Annotate {
                    actor: 183,
                    pos: 183,
                    len: 187,
                    annotation: Bold,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 255,
                    pos: 255,
                    len: 255,
                },
                Sync(255, 255),
                Sync(255, 255),
                Insert {
                    actor: 63,
                    pos: 0,
                    content: 0,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 0,
                    annotation: Link,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 255,
                    pos: 223,
                    len: 255,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 91,
                    len: 72,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Annotate {
                    actor: 183,
                    pos: 183,
                    len: 187,
                    annotation: Bold,
                },
                Delete {
                    actor: 91,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 255,
                    len: 255,
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
                    actor: 255,
                    pos: 255,
                    len: 255,
                },
                Sync(255, 255),
                Sync(255, 255),
                Insert {
                    actor: 63,
                    pos: 0,
                    content: 0,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 0,
                    annotation: Link,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 255,
                    pos: 223,
                    len: 255,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 91,
                    len: 72,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Annotate {
                    actor: 183,
                    pos: 183,
                    len: 187,
                    annotation: Bold,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 255,
                    pos: 255,
                    len: 255,
                },
                Sync(255, 255),
                Sync(255, 255),
                Insert {
                    actor: 63,
                    pos: 0,
                    content: 0,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 0,
                    annotation: Link,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 255,
                    pos: 223,
                    len: 255,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 91,
                    len: 72,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Annotate {
                    actor: 183,
                    pos: 183,
                    len: 187,
                    annotation: Bold,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 5,
                    pos: 72,
                    len: 161,
                },
                Delete {
                    actor: 104,
                    pos: 72,
                    len: 72,
                },
                Sync(5, 63),
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 32896,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Annotate {
                    actor: 72,
                    pos: 45,
                    len: 72,
                    annotation: Comment,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 1,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Annotate {
                    actor: 178,
                    pos: 178,
                    len: 178,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 178,
                    pos: 178,
                    len: 178,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 178,
                    pos: 178,
                    len: 178,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 178,
                    pos: 255,
                    len: 255,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 185,
                    pos: 72,
                    len: 72,
                    annotation: Bold,
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
                    len: 206,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 185,
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
                    actor: 72,
                    pos: 72,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 185,
                    len: 72,
                },
                Sync(255, 255),
                Sync(255, 255),
                Sync(255, 255),
                Sync(128, 128),
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Delete {
                    actor: 45,
                    pos: 72,
                    len: 91,
                },
                Insert {
                    actor: 129,
                    pos: 0,
                    content: 0,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 191,
                    len: 72,
                },
                Annotate {
                    actor: 72,
                    pos: 72,
                    len: 72,
                    annotation: UnLink,
                },
                Sync(255, 255),
                Sync(255, 255),
                Sync(255, 255),
                Insert {
                    actor: 255,
                    pos: 255,
                    content: 65535,
                },
                Annotate {
                    actor: 183,
                    pos: 183,
                    len: 183,
                    annotation: Bold,
                },
                Annotate {
                    actor: 72,
                    pos: 72,
                    len: 72,
                    annotation: Bold,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 206,
                    pos: 1,
                    len: 0,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Annotate {
                    actor: 72,
                    pos: 72,
                    len: 184,
                    annotation: Bold,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 91,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Sync(255, 255),
                Sync(255, 255),
                Sync(255, 255),
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 255,
                    pos: 223,
                    len: 255,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 91,
                    len: 72,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Annotate {
                    actor: 183,
                    pos: 183,
                    len: 187,
                    annotation: Bold,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 255,
                    pos: 255,
                    len: 255,
                },
                Sync(255, 255),
                Sync(255, 255),
                Insert {
                    actor: 63,
                    pos: 0,
                    content: 0,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 0,
                    annotation: Link,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 255,
                    pos: 223,
                    len: 255,
                },
                Annotate {
                    actor: 128,
                    pos: 128,
                    len: 128,
                    annotation: Comment,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 91,
                    len: 72,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Annotate {
                    actor: 183,
                    pos: 183,
                    len: 187,
                    annotation: Bold,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Annotate {
                    actor: 178,
                    pos: 178,
                    len: 178,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 178,
                    pos: 178,
                    len: 178,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 178,
                    pos: 178,
                    len: 178,
                    annotation: UnBold,
                },
                Annotate {
                    actor: 178,
                    pos: 178,
                    len: 178,
                    annotation: UnLink,
                },
                Sync(255, 255),
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 3840,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 65280,
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
                    actor: 0,
                    pos: 0,
                    content: 22222,
                },
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 33333,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    content: 44444,
                },
                Delete {
                    actor: 1,
                    pos: 4,
                    len: 1,
                },
            ],
        )
    }

    #[test]
    fn fuzz_12() {
        fuzzing(
            3,
            vec![
                Insert {
                    actor: 2,
                    pos: 0,
                    content: 2,
                },
                Sync(0, 2),
                Insert {
                    actor: 1,
                    pos: 0,
                    content: 1,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
            ],
        )
    }

    #[test]
    fn fuzz_13() {
        fuzzing(
            5,
            vec![
                Insert {
                    actor: 4,
                    pos: 0,
                    content: 44,
                },
                Sync(2, 4),
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Sync(0, 4),
                Insert {
                    actor: 2,
                    pos: 2,
                    content: 2,
                },
                Insert {
                    actor: 0,
                    pos: 3,
                    content: 1,
                },
                Sync(4, 0),
                Insert {
                    actor: 4,
                    pos: 3,
                    content: 55,
                },
            ],
        )
    }

    #[test]
    fn fuzz_14() {
        fuzzing(
            5,
            vec![
                Insert {
                    actor: 4,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    content: 256,
                },
                Sync(0, 1),
                Sync(0, 4),
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 1,
                    pos: 1,
                    content: 0,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Sync(0, 1),
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
            ],
        )
    }

    #[test]
    fn fuzz_15() {
        fuzzing(
            5,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 38,
                },
                Sync(1, 0),
                Insert {
                    actor: 1,
                    pos: 0,
                    content: 256,
                },
                Sync(0, 1),
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Sync(1, 0),
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 1,
                    pos: 1,
                    content: 0,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Sync(0, 1),
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
            ],
        );
    }

    #[test]
    fn fuzz_16() {
        fuzzing_match_str(vec![
            Insert {
                actor: 2,
                pos: 252,
                content: 54247,
            },
            Insert {
                actor: 252,
                pos: 231,
                content: 54042,
            },
            Insert {
                actor: 67,
                pos: 63,
                content: 17219,
            },
            Insert {
                actor: 0,
                pos: 0,
                content: 17219,
            },
            Annotate {
                actor: 79,
                pos: 79,
                len: 79,
                annotation: Link,
            },
            Delete {
                actor: 79,
                pos: 79,
                len: 79,
            },
            Delete {
                actor: 133,
                pos: 79,
                len: 79,
            },
        ])
    }

    #[test]
    fn fuzz_17() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 22222,
                },
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 3,
                    content: 33333,
                },
                Insert {
                    actor: 1,
                    pos: 3,
                    content: 111,
                },
                Sync(0, 1),
                Annotate {
                    actor: 0,
                    pos: 1,
                    len: 10,
                    annotation: Link,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 44444,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 55555,
                },
                Sync(1, 0),
                Sync(0, 1),
                Insert {
                    actor: 1,
                    pos: 21,
                    content: 6666,
                },
            ],
        )
    }

    #[test]
    fn minimize() {
        let actions = vec![];
        minify_error(
            5,
            actions,
            |n, actions| fuzzing(n as usize, actions.to_vec()),
            |_, x| x.to_vec(),
        );
    }
}
