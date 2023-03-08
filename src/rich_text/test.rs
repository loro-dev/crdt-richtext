use super::*;
mod insert;

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
}
