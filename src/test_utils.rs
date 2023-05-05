use std::collections::HashSet;

use super::*;
use arbitrary::Arbitrary;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SimpleSpan {
    pub len: usize,
    pub annotations: HashSet<InternalString>,
}

#[derive(Arbitrary, Clone, Copy, Debug)]
pub enum AnnotationType {
    Link,
    Bold,
    Comment,
    UnBold,
    UnLink,
}

#[derive(Arbitrary, Clone, Debug, Copy)]
pub enum Action {
    Insert {
        actor: u8,
        pos: u8,
        len: u8,
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

#[allow(unused)]
pub(crate) fn make_spans(spans: &[(Vec<&str>, usize)]) -> Vec<SimpleSpan> {
    spans
        .iter()
        .map(|(annotations, len)| SimpleSpan {
            annotations: annotations.iter().map(|x| (*x).into()).collect(),
            len: *len,
        })
        .collect()
}
