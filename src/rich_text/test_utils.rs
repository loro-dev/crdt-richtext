use std::{cell::RefCell, rc::Rc};

use crate::{test_utils::AnnotationType, InternalString};

use super::*;
use arbitrary::Arbitrary;

mod fuzz_line_breaks;
pub use fuzz_line_breaks::{fuzzing_line_break, Action as LineBreakFuzzAction};

pub struct Actor {
    pub text: RichText,
}

#[derive(Arbitrary, Clone, Debug, Copy)]
pub enum Action {
    Insert {
        actor: u8,
        pos: u8,
        content: u16,
    },
    Delete {
        actor: u8,
        pos: u8,
        len: u8,
    },
    Annotate {
        actor: u8,
        pos: u8,
        len: u8,
        annotation: AnnotationType,
    },
    Sync(u8, u8),
}

pub fn preprocess_action(actors: &[Actor], action: &mut Action) {
    match action {
        Action::Insert {
            actor,
            pos,
            content: _,
        } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len() + 1)) as u8;
        }
        Action::Delete { actor, pos, len } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len() + 1)) as u8;
            *len = (*len).min(10);
            *len %= (actors[*actor as usize].len().max(*pos as usize + 1) - *pos as usize)
                .min(255)
                .max(1) as u8;
        }
        Action::Annotate {
            actor,
            pos,
            len,
            annotation: _,
        } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len() + 1)) as u8;
            *len = (*len).min(10);
            *len %= (actors[*actor as usize].len().max(*pos as usize + 1) - *pos as usize)
                .min(255)
                .max(1) as u8;
        }
        Action::Sync(a, b) => {
            *a %= actors.len() as u8;
            *b %= actors.len() as u8;
            if b == a {
                *b = (*a + 1) % actors.len() as u8;
            }
        }
    }
}

pub fn preprocess_action_utf16(actors: &[Actor], action: &mut Action) {
    match action {
        Action::Insert {
            actor,
            pos,
            content: _,
        } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len_utf16() + 1)) as u8;
        }
        Action::Delete { actor, pos, len } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len_utf16() + 1)) as u8;
            *len = (*len).min(10);
            *len %= (actors[*actor as usize].len_utf16().max(*pos as usize + 1) - *pos as usize)
                .min(255)
                .max(1) as u8;
        }
        Action::Annotate {
            actor,
            pos,
            len,
            annotation: _,
        } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len_utf16() + 1)) as u8;
            *len = (*len).min(10);
            *len %= (actors[*actor as usize].len_utf16().max(*pos as usize + 1) - *pos as usize)
                .min(255)
                .max(1) as u8;
        }
        Action::Sync(a, b) => {
            *a %= actors.len() as u8;
            *b %= actors.len() as u8;
            if b == a {
                *b = (*a + 1) % actors.len() as u8;
            }
        }
    }
}

pub fn apply_action(actors: &mut [Actor], action: Action) {
    match action {
        Action::Insert {
            actor,
            pos,
            content,
        } => {
            actors[actor as usize].insert(pos as usize, content.to_string().as_str());
            // actors[actor as usize].check();
        }
        Action::Delete { actor, pos, len } => {
            if len == 0 {
                return;
            }

            actors[actor as usize].delete(pos as usize, len as usize);
            // actors[actor as usize].check();
        }
        Action::Annotate {
            actor,
            pos,
            len,
            annotation,
        } => {
            if len == 0 {
                return;
            }

            actors[actor as usize].annotate(pos as usize..pos as usize + len as usize, annotation);
            // actors[actor as usize].check();
        }
        Action::Sync(a, b) => {
            let (a, b) = arref::array_mut_ref!(actors, [a as usize, b as usize]);
            a.merge(b);
            a.text.debug_log(true);
            // a.check();
        }
    }
}

pub fn apply_action_utf16(actors: &mut [Actor], action: Action) {
    match action {
        Action::Insert {
            actor,
            pos,
            content,
        } => {
            actors[actor as usize].insert_utf16(pos as usize, content.to_string().as_str());
            // actors[actor as usize].check();
        }
        Action::Delete { actor, pos, len } => {
            if len == 0 {
                return;
            }

            actors[actor as usize].delete_utf16(pos as usize, len as usize);
            // actors[actor as usize].check();
        }
        Action::Annotate {
            actor,
            pos,
            len,
            annotation,
        } => {
            if len == 0 {
                return;
            }

            actors[actor as usize]
                .annotate_utf16(pos as usize..pos as usize + len as usize, annotation);
            // actors[actor as usize].check();
        }
        Action::Sync(a, b) => {
            let (a, b) = arref::array_mut_ref!(actors, [a as usize, b as usize]);
            a.merge(b);
            // a.check();
        }
    }
}

pub fn fuzzing(actor_num: usize, actions: Vec<Action>) {
    let mut actors = vec![];
    for i in 0..actor_num {
        actors.push(Actor::new(i));
    }

    for mut action in actions {
        preprocess_action(&actors, &mut action);
        // println!("{:?},", &action);
        debug_log::group!("{:?},", &action);
        apply_action(&mut actors, action);
        debug_log::group_end!();
    }

    for i in 0..actors.len() {
        for j in (i + 1)..actors.len() {
            let (a, b) = arref::array_mut_ref!(&mut actors, [i, j]);
            debug_log::group!("merge {i}<-{j}");
            a.merge(b);
            debug_log::group_end!();
            debug_log::group!("merge {i}->{j}");
            b.merge(a);
            assert_eq!(a.text.get_spans(), b.text.get_spans());
            debug_log::group_end!();
        }
    }
}

pub fn fuzzing_utf16(actor_num: usize, actions: Vec<Action>) {
    let mut actors = vec![];
    let followers = vec![
        Rc::new(RefCell::new(String::new())),
        Rc::new(RefCell::new(String::new())),
    ];
    for i in 0..actor_num {
        if i <= 1 {
            let mut actor = Actor::new(i);
            let f = followers[i].clone();
            actor.text.observe(Box::new(move |event| {
                let mut index = 0;
                for op in event.ops.iter() {
                    match op {
                        crate::rich_text::delta::DeltaItem::Retain { retain, .. } => {
                            index += *retain;
                        }
                        crate::rich_text::delta::DeltaItem::Insert { insert, .. } => {
                            f.borrow_mut().insert_str(index, insert);
                            index += insert.len();
                        }
                        crate::rich_text::delta::DeltaItem::Delete { delete } => {
                            f.borrow_mut().drain(index..index + *delete);
                        }
                    }
                }
            }));

            actors.push(actor);
        } else {
            actors.push(Actor::new(i));
        }
    }

    for mut action in actions {
        preprocess_action_utf16(&actors, &mut action);
        // println!("{:?},", &action);
        debug_log::group!("{:?},", &action);
        apply_action_utf16(&mut actors, action);
        debug_log::group_end!();
    }

    for i in 0..actors.len() {
        for j in (i + 1)..actors.len() {
            let (a, b) = arref::array_mut_ref!(&mut actors, [i, j]);
            debug_log::group!("merge {i}<-{j}");
            a.merge(b);
            debug_log::group_end!();
            debug_log::group!("merge {i}->{j}");
            b.merge(a);
            debug_log::group_end!();
            assert_eq!(a.text.get_spans(), b.text.get_spans());
            if i <= 1 {
                assert_eq!(a.text.to_string(), followers[i].borrow().to_string());
            }
        }
    }
}

pub fn fuzzing_match_str(actions: Vec<Action>) {
    let word_choices: [InternalString; 8] = [
        "a".into(),
        "b".into(),
        "c".into(),
        "d".into(),
        "一".into(),
        "二".into(),
        "三".into(),
        "四".into(),
    ];
    let mut actor = Actor::new(1);
    let mut s: Vec<InternalString> = vec![];
    for action in actions {
        if matches!(action, Action::Sync(_, _) | Action::Annotate { .. }) {
            continue;
        }

        match action {
            Action::Insert { pos, content, .. } => {
                let mut pos = pos as usize;
                if s.is_empty() {
                    pos = 0;
                } else {
                    pos %= s.len();
                }

                let content = &word_choices[content as usize % word_choices.len()];
                s.insert(pos, content.clone());
                debug_log::group!(
                    "INSERT pos={} content={} ans={}",
                    pos,
                    content,
                    s.iter().fold(String::new(), |mut left, cur| {
                        left.push_str(cur);
                        left
                    })
                );
                actor.insert_utf16(pos, content);
                // actor.check();
            }
            Action::Delete { pos, len, .. } => {
                let mut pos = pos as usize;
                if s.is_empty() {
                    pos = 0;
                } else {
                    pos %= s.len();
                }
                let len = (len as usize).min(s.len() - pos);
                s.drain(pos..pos + len);
                debug_log::group!(
                    "DELETE pos={} len={} ans={}",
                    pos,
                    len,
                    s.iter().fold(String::new(), |mut left, cur| {
                        left.push_str(cur);
                        left
                    })
                );
                actor.delete_utf16(pos, len);
                // actor.check();
            }
            _ => {}
        }

        debug_log::group_end!();
    }

    let mut ans = String::new();
    for span in s {
        ans.push_str(&span)
    }

    assert_eq!(&actor.text.to_string(), &ans)
}

impl Actor {
    pub fn new(id: usize) -> Self {
        Self {
            text: RichText::new(id as u64),
        }
    }

    pub fn insert(&mut self, pos: usize, content: &str) {
        self.text.insert(pos, content);
    }

    pub fn insert_utf16(&mut self, pos: usize, as_str: &str) {
        self.text.insert_utf16(pos, as_str)
    }

    /// this should happen after the op is integrated to the list crdt
    pub fn delete(&mut self, pos: usize, len: usize) {
        self.text.delete(pos..pos + len)
    }

    pub fn delete_utf16(&mut self, pos: usize, len: usize) {
        self.text.delete_utf16(pos..pos + len)
    }

    pub fn annotate(&mut self, range: impl RangeBounds<usize>, type_: AnnotationType) {
        self._annotate(range, type_, IndexType::Utf8)
    }

    pub fn annotate_utf16(&mut self, range: impl RangeBounds<usize>, type_: AnnotationType) {
        self._annotate(range, type_, IndexType::Utf16)
    }

    fn _annotate(
        &mut self,
        range: impl RangeBounds<usize>,
        type_: AnnotationType,
        index_type: IndexType,
    ) {
        match type_ {
            AnnotationType::Bold => self.text.annotate_inner(
                range,
                Style {
                    expand: Expand::After,
                    behavior: crate::Behavior::Merge,
                    type_: "bold".into(),
                    value: serde_json::Value::Null,
                },
                index_type,
            ),
            AnnotationType::Link => self.text.annotate_inner(
                range,
                Style {
                    expand: Expand::None,
                    behavior: crate::Behavior::Merge,
                    type_: "link".into(),
                    value: serde_json::Value::Bool(true),
                },
                index_type,
            ),
            AnnotationType::Comment => self.text.annotate_inner(
                range,
                Style {
                    expand: Expand::None,
                    behavior: crate::Behavior::AllowMultiple,
                    type_: "comment".into(),
                    value: serde_json::Value::String("This is a comment".to_owned()),
                },
                index_type,
            ),
            AnnotationType::UnBold => self.text.annotate_inner(
                range,
                Style {
                    expand: Expand::After,
                    behavior: crate::Behavior::Delete,
                    type_: "bold".into(),
                    value: serde_json::Value::Null,
                },
                index_type,
            ),
            AnnotationType::UnLink => self.text.annotate_inner(
                range,
                Style {
                    expand: Expand::Both,
                    behavior: crate::Behavior::Delete,
                    type_: "link".into(),
                    value: serde_json::Value::Null,
                },
                index_type,
            ),
        };
    }

    fn merge(&mut self, other: &Self) {
        self.text.merge(&other.text)
    }

    #[allow(unused)]
    fn check_eq(&mut self, other: &mut Self) {
        assert_eq!(self.len(), other.len());
        assert_eq!(self.text.to_string(), other.text.to_string());
    }

    #[allow(unused)]
    fn len(&self) -> usize {
        self.text.len()
    }

    #[allow(unused)]
    fn len_utf16(&self) -> usize {
        self.text.len_utf16()
    }

    fn check(&self) {
        self.text.check()
    }
}
