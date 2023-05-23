use std::{cell::RefCell, rc::Rc};

use arbitrary::Arbitrary;

use crate::RichText;

#[derive(Arbitrary, Clone, Debug, Copy)]
pub enum Action {
    Insert {
        index: u16,
        content: u16,
        has_line_break: bool,
    },
    Delete {
        index: u16,
        len: u8,
    },
}

fn preprocess(actions: &mut [Action]) {
    let mut len: usize = 0;
    for action in actions.iter_mut() {
        match action {
            Action::Insert {
                index,
                content,
                has_line_break,
            } => {
                *index = ((*index as usize) % (len + 1)) as u16;
                len += content.to_string().len();
                if *has_line_break {
                    len += 1;
                }
            }
            Action::Delete {
                index,
                len: del_len,
            } => {
                if len == 0 {
                    *del_len = 0;
                    *index = 0;
                } else {
                    *index = ((*index as usize) % len) as u16;
                    *del_len = ((*del_len as usize) % (len - *index as usize)) as u8;
                }
                len -= *del_len as usize;
            }
        }
    }
}

fn apply(text: &mut RichText, actions: &[Action]) {
    for action in actions.iter() {
        match action {
            Action::Insert {
                index,
                content,
                has_line_break,
            } => {
                debug_log::group!(
                    "insert index={} content={} linebreak={}",
                    index,
                    content,
                    has_line_break
                );
                let mut content = content.to_string();
                if *has_line_break {
                    content.push('\n');
                }
                text.insert(*index as usize, &content);
                // text.check();
                debug_log::group_end!();
            }
            Action::Delete { index, len } => {
                debug_log::group!("delete index={} len={}", index, len);
                text.debug_log(true);
                text.delete(*index as usize..*index as usize + *len as usize);
                // text.check();
                debug_log::group_end!();
            }
        }
    }
}

fn apply_to_str(actions: &[Action]) -> String {
    let mut ans = String::new();
    for action in actions.iter() {
        match action {
            Action::Insert {
                index,
                content,
                has_line_break,
            } => {
                let mut content = content.to_string();
                if *has_line_break {
                    content.push('\n');
                }
                ans.insert_str(*index as usize, &content);
            }
            Action::Delete { index, len } => {
                ans.drain(*index as usize..*index as usize + *len as usize);
            }
        }
    }

    ans
}

pub fn fuzzing_line_break(mut actions: Vec<Action>) {
    let mut rich_text = RichText::new(1);
    let follower = String::new();
    let follower_ref = Rc::new(RefCell::new(follower));
    let follower_bk = Rc::clone(&follower_ref);
    rich_text.observe(Box::new(move |event| {
        let mut index = 0;
        for op in event.ops.iter() {
            match op {
                crate::rich_text::delta::DeltaItem::Retain { retain, .. } => {
                    index += *retain;
                }
                crate::rich_text::delta::DeltaItem::Insert { insert, .. } => {
                    follower_ref.borrow_mut().insert_str(index, insert);
                    index += insert.len();
                }
                crate::rich_text::delta::DeltaItem::Delete { delete } => {
                    follower_ref.borrow_mut().drain(index..index + *delete);
                }
            }
        }
    }));

    preprocess(&mut actions);
    debug_log::debug_dbg!("actions: {:?}", &actions);
    apply(&mut rich_text, &actions);
    let s = apply_to_str(&actions);
    assert_eq!(rich_text.to_string(), s);
    assert_eq!(&follower_bk.borrow().as_str(), &s);
    if rich_text.is_empty() {
        assert!(s.is_empty());
        return;
    }
    debug_log::debug_dbg!("{:?}", &s);
    rich_text.debug_log(true);
    for (ln, str) in s.split('\n').enumerate() {
        assert_eq!(&rich_text.get_line(ln)[0].insert.trim(), &str);
    }
}

mod test {

    use super::*;
    use Action::*;

    #[test]
    fn test() {
        fuzzing_line_break(vec![]);
        fuzzing_line_break(vec![Action::Insert {
            index: 0,
            content: 1,
            has_line_break: false,
        }]);
        fuzzing_line_break(vec![Action::Delete {
            index: 10,
            len: 100,
        }]);
        fuzzing_line_break(vec![
            Action::Insert {
                index: 0,
                content: 123,
                has_line_break: true,
            },
            Action::Delete {
                index: 10,
                len: 100,
            },
        ]);
    }

    #[test]
    fn fuzz_0() {
        fuzzing_line_break(vec![
            Insert {
                index: 0,
                content: 256,
                has_line_break: false,
            },
            Delete {
                index: 5911,
                len: 23,
            },
            Insert {
                index: 5911,
                content: 5911,
                has_line_break: true,
            },
        ])
    }

    #[test]
    fn fuzz_1() {
        fuzzing_line_break(vec![
            Insert {
                index: 0,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 3,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 10,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 11,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 3,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 29,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 27,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 7,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 17,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 8,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 51,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 2,
                content: 20303,
                has_line_break: true,
            },
            Insert {
                index: 1,
                content: 30720,
                has_line_break: false,
            },
            Insert {
                index: 12,
                content: 0,
                has_line_break: false,
            },
            Insert {
                index: 76,
                content: 323,
                has_line_break: true,
            },
            Delete { index: 22, len: 23 },
            Insert {
                index: 31,
                content: 30840,
                has_line_break: false,
            },
            Insert {
                index: 59,
                content: 17219,
                has_line_break: true,
            },
        ])
    }

    #[test]
    fn fuzz_2() {
        fuzzing_line_break(vec![
            Insert {
                index: 74,
                content: 10752,
                has_line_break: false,
            },
            Delete {
                index: 2056,
                len: 8,
            },
            Insert {
                index: 5911,
                content: 5911,
                has_line_break: true,
            },
            Insert {
                index: 2056,
                content: 2056,
                has_line_break: false,
            },
            Insert {
                index: 2602,
                content: 17194,
                has_line_break: false,
            },
            Insert {
                index: 5911,
                content: 16407,
                has_line_break: true,
            },
            Insert {
                index: 2056,
                content: 2056,
                has_line_break: false,
            },
            Insert {
                index: 2570,
                content: 10794,
                has_line_break: false,
            },
            Insert {
                index: 10794,
                content: 10762,
                has_line_break: true,
            },
            Insert {
                index: 2583,
                content: 0,
                has_line_break: false,
            },
            Insert {
                index: 10752,
                content: 0,
                has_line_break: false,
            },
            Insert {
                index: 23594,
                content: 23644,
                has_line_break: false,
            },
            Insert {
                index: 11007,
                content: 23644,
                has_line_break: false,
            },
            Insert {
                index: 65372,
                content: 65535,
                has_line_break: true,
            },
            Insert {
                index: 23644,
                content: 10844,
                has_line_break: true,
            },
            Insert {
                index: 2071,
                content: 520,
                has_line_break: false,
            },
            Insert {
                index: 0,
                content: 65288,
                has_line_break: true,
            },
            Insert {
                index: 23644,
                content: 23644,
                has_line_break: false,
            },
            Insert {
                index: 5911,
                content: 2056,
                has_line_break: false,
            },
            Delete {
                index: 248,
                len: 10,
            },
        ])
    }

    #[test]
    fn fuzz_3() {
        fuzzing_line_break(vec![
            Insert {
                index: 74,
                content: 10752,
                has_line_break: false,
            },
            Delete {
                index: 2056,
                len: 8,
            },
            Insert {
                index: 5911,
                content: 5911,
                has_line_break: true,
            },
            Insert {
                index: 255,
                content: 0,
                has_line_break: false,
            },
            Insert {
                index: 2602,
                content: 17194,
                has_line_break: false,
            },
            Insert {
                index: 5911,
                content: 16407,
                has_line_break: true,
            },
            Insert {
                index: 2056,
                content: 2056,
                has_line_break: false,
            },
            Insert {
                index: 2570,
                content: 10794,
                has_line_break: false,
            },
            Insert {
                index: 10794,
                content: 10762,
                has_line_break: true,
            },
            Insert {
                index: 2583,
                content: 0,
                has_line_break: false,
            },
            Insert {
                index: 10752,
                content: 17152,
                has_line_break: true,
            },
            Insert {
                index: 67,
                content: 0,
                has_line_break: false,
            },
            Insert {
                index: 0,
                content: 0,
                has_line_break: false,
            },
            Insert {
                index: 23644,
                content: 23644,
                has_line_break: false,
            },
            Insert {
                index: 23644,
                content: 23644,
                has_line_break: false,
            },
            Delete {
                index: 65535,
                len: 255,
            },
            Insert {
                index: 23644,
                content: 5930,
                has_line_break: true,
            },
            Insert {
                index: 2056,
                content: 2,
                has_line_break: false,
            },
            Insert {
                index: 2048,
                content: 65535,
                has_line_break: true,
            },
            Insert {
                index: 23644,
                content: 10844,
                has_line_break: true,
            },
            Insert {
                index: 2071,
                content: 2056,
                has_line_break: false,
            },
            Delete {
                index: 2560,
                len: 42,
            },
        ])
    }

    #[test]
    fn fuzz_4() {
        fuzzing_line_break(vec![
            Insert {
                index: 0,
                content: 16896,
                has_line_break: true,
            },
            Insert {
                index: 48451,
                content: 65468,
                has_line_break: true,
            },
            Delete {
                index: 27278,
                len: 255,
            },
            Delete {
                index: 17152,
                len: 72,
            },
        ])
    }

    #[test]
    fn fuzz_5() {
        fuzzing_line_break(vec![
            Insert {
                index: 17219,
                content: 16195,
                has_line_break: true,
            },
            Delete {
                index: 60652,
                len: 231,
            },
            Delete { index: 2, len: 67 },
            Delete {
                index: 63489,
                len: 236,
            },
        ])
    }

    #[test]
    fn fuzz_6() {
        fuzzing_line_break(vec![
            Insert {
                index: 0,
                content: 16896,
                has_line_break: true,
            },
            Insert {
                index: 48451,
                content: 65468,
                has_line_break: true,
            },
            Insert {
                index: 3584,
                content: 17210,
                has_line_break: true,
            },
            Insert {
                index: 17152,
                content: 1,
                has_line_break: false,
            },
            Insert {
                index: 64652,
                content: 30019,
                has_line_break: false,
            },
            Delete {
                index: 65535,
                len: 255,
            },
            Insert {
                index: 74,
                content: 37418,
                has_line_break: false,
            },
            Insert {
                index: 17152,
                content: 72,
                has_line_break: false,
            },
            Insert {
                index: 257,
                content: 0,
                has_line_break: false,
            },
            Delete {
                index: 65532,
                len: 255,
            },
        ])
    }
}
