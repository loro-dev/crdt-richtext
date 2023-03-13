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
}

mod annotation {
    use crate::{AnchorType, InternalString};

    use super::*;

    fn bold() -> Style {
        Style {
            start_type: AnchorType::Before,
            end_type: AnchorType::Before,
            merge_method: crate::RangeMergeRule::Merge,
            type_: InternalString::from("bold"),
        }
    }

    fn link() -> Style {
        Style {
            start_type: AnchorType::Before,
            end_type: AnchorType::After,
            merge_method: crate::RangeMergeRule::Merge,
            type_: InternalString::from("bold"),
        }
    }

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
    }
}
