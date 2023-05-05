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
        text.insert(0, "1");
        assert_eq!(text.utf16_len(), 1);
        text.insert(1, "2");
        assert_eq!(text.utf16_len(), 2);
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
        assert_eq!(span.right, None);
        let span = text.content.iter().nth(1).unwrap();
        assert_eq!(span.rle_len(), 1);
        assert_eq!(span.right.unwrap().counter, 1);
    }
}
