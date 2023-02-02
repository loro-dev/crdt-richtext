use std::{collections::BTreeMap, ops::Range, sync::Arc};

use crate::{Annotation, OpID};

/// the position of annotation relative to its owner span
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnnPos {
    pub begin_here: bool,
    pub end_here: bool,
}

/// the position of span relative to a new insert
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativeSpanPos {
    Before,
    Middle,
    After,
}

/// the position of annotation relative to a new insert
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnPosRelativeToInsert {
    EndBeforeInsert,
    StartAfterInsert,
    IncludeInsert,
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
    fn insert<F>(&mut self, pos: usize, len: usize, f: F)
    where
        F: FnMut(&Annotation, AnnPos, RelativeSpanPos) -> AnnPosRelativeToInsert;
    fn insert_directly(&mut self, pos: usize, len: usize) {
        self.insert(pos, len, |_, _, _| AnnPosRelativeToInsert::IncludeInsert);
    }
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
pub mod dumb_impl;
