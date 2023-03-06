use std::{collections::BTreeSet, ops::Range, sync::Arc};
mod small_set;
pub mod tree_impl;

use crate::{Annotation, Lamport, OpID};

pub trait RangeMap {
    fn init() -> Self;
    /// f is used to position the annotations when they ends in the insert range
    fn insert<F>(&mut self, pos: usize, len: usize, f: F)
    where
        F: FnMut(&Annotation) -> AnnPosRelativeToInsert;
    fn insert_directly(&mut self, pos: usize, len: usize) {
        self.insert(pos, len, |_| AnnPosRelativeToInsert::IncludeInsert);
    }
    fn delete(&mut self, pos: usize, len: usize);
    fn annotate(&mut self, pos: usize, len: usize, annotation: Annotation);
    /// should keep the shrink annotations around even if they are deleted completely
    fn adjust_annotation(
        &mut self,
        target_id: OpID,
        lamport: Lamport,
        patch_id: OpID,
        start_shift: Option<(isize, Option<OpID>)>,
        end_shift: Option<(isize, Option<OpID>)>,
    );
    fn delete_annotation(&mut self, id: OpID);
    /// TODO: need to clarify the rules when encounter an empty span on the edges
    fn get_annotations(&mut self, pos: usize, len: usize) -> Vec<Span>;
    fn get_annotation_pos(&self, id: OpID) -> Option<(Arc<Annotation>, Range<usize>)>;
    fn len(&self) -> usize;
}

/// the position of annotation relative to a new insert
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnPosRelativeToInsert {
    Before,
    After,
    IncludeInsert,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub annotations: BTreeSet<Arc<Annotation>>,
    pub len: usize,
}

impl Span {
    pub fn new(len: usize) -> Self {
        Span {
            annotations: BTreeSet::new(),
            len,
        }
    }
}

#[cfg(feature = "test")]
pub mod dumb_impl;
