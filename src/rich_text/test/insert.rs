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
}
