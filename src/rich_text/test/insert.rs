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
