use std::{collections::BTreeMap, ops::Range, sync::Arc};

use crate::{Annotation, OpID};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnnPos {
    pub begin_here: bool,
    pub end_here: bool,
}

impl AnnPos {
    pub fn merge(&mut self, other: &Self) {
        self.begin_here = self.begin_here || other.begin_here;
        self.end_here = self.end_here || other.end_here;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub annotations: BTreeMap<Arc<Annotation>, AnnPos>,
    pub len: usize,
}

impl Span {
    pub fn new(len: usize) -> Self {
        Span {
            annotations: BTreeMap::new(),
            len,
        }
    }

    pub fn update_pos(&mut self, begin: Option<bool>, end: Option<bool>) {
        for ann in self.annotations.iter_mut() {
            if let Some(begin) = begin {
                ann.1.begin_here = begin;
            }
            if let Some(end) = end {
                ann.1.end_here = end;
            }
        }
    }
}

pub trait RangeMap {
    fn init() -> Self;
    fn insert(
        &mut self,
        pos: usize,
        len: usize,
        annotations: Option<BTreeMap<Arc<Annotation>, AnnPos>>,
    );
    fn delete(&mut self, pos: usize, len: usize);
    fn annotate(&mut self, pos: usize, len: usize, annotation: Annotation);
    fn expand_annotation(&mut self, id: OpID, len: usize, reverse: bool);
    fn shrink_annotation(&mut self, id: OpID, len: usize);
    fn delete_annotation(&mut self, id: OpID);
    fn get_annotations(&self, pos: usize, len: usize) -> Vec<Span>;
    fn get_annotation_pos(&self, id: OpID) -> Option<(Arc<Annotation>, Range<usize>)>;
    fn len(&self) -> usize;
}

#[cfg(feature = "test")]
pub mod test;
