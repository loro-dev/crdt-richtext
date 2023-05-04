//! This Rust crate provides an implementation of Peritext that is optimized for
//! performance. This crate uses a separate data structure to store the range
//! annotation, decoupled from the underlying list CRDT. This implementation depends
//! on `RangeMap` trait, which can be implemented efficiently to make the overall
//! algorithm fast.
//!
//! This implementation provides another property to the algorithm: *all local operations' effects
//! can be calculated without using CRDTs*. (This requires List CRDT will not insert new elements
//! into the middle of tombstones).
//!
//!
//!

#![deny(unsafe_code)]

use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashMap},
    fmt::Debug,
    ops::{Bound, Range, RangeBounds},
    sync::Arc,
};

pub use range_map::tree_impl::TreeRangeMap;
pub use range_map::RangeMap;
use range_map::{AnnPosRelativeToInsert, Span};
use string_cache::DefaultAtom;

mod range_map;
pub mod rich_text;
pub(crate) type InternalString = DefaultAtom;
type Lamport = u32;
type ClientID = u64;
type Counter = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpID {
    client: ClientID,
    counter: Counter,
}

impl OpID {
    pub fn inc(&self, inc: Counter) -> Self {
        Self {
            client: self.client,
            counter: self.counter + inc as Counter,
        }
    }

    pub fn inc_i32(&self, inc: i32) -> Self {
        if inc > 0 {
            Self {
                client: self.client,
                counter: self.counter + inc as Counter,
            }
        } else {
            let (mut counter, overflow) = self.counter.overflowing_sub((-inc) as Counter);
            if overflow {
                counter = Counter::MAX;
            }

            Self {
                client: self.client,
                counter,
            }
        }
    }
}

pub(crate) struct IdSpan {
    id: OpID,
    len: Counter,
}

impl IdSpan {
    pub fn new(id: OpID, len: usize) -> Self {
        Self {
            id,
            len: len as Counter,
        }
    }

    pub fn contains(&self, id: OpID) -> bool {
        self.id.client == id.client
            && self.id.counter <= id.counter
            && id.counter < self.id.counter + self.len
    }
}

#[derive(Debug, Clone)]
pub enum RangeOp {
    Patch(Patch),
    Annotate(Annotation),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnchorType {
    Before,
    After,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Behavior {
    /// When calculating the final state, it will keep all the ranges even if they have the same type
    ///
    /// For example, we would like to keep both comments alive even if they have overlapped regions
    Inclusive,
    /// When calculating the final state, it will merge the ranges that have overlapped regions and have the same type
    ///
    /// For example, [bold 2~5] can be merged with [bold 1~4] to produce [bold 1-5]
    Merge,
    /// It will delete the overlapped range that has smaller lamport && has the same type
    Delete,
}

/// If both `move_start_to` and `move_end_to` equal to None, the target range will be deleted
#[derive(Clone, Copy, Debug)]
pub struct Patch {
    pub id: OpID,
    pub target_range_id: OpID,
    pub move_start_to: Option<OpID>,
    pub move_end_to: Option<OpID>,
    pub lamport: Lamport,
}

#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub struct Annotation {
    pub id: OpID,
    /// lamport value of the current range (it may be updated by patch)
    pub range_lamport: (Lamport, OpID),
    pub range: AnchorRange,
    pub behavior: Behavior,
    /// "bold", "comment", "italic", etc.
    pub type_: InternalString,
    pub meta: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    pub start_type: AnchorType,
    pub end_type: AnchorType,
    pub behavior: Behavior,
    /// "bold", "comment", "italic", etc.
    pub type_: InternalString,
}

#[derive(Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct AnchorRange {
    pub start: Anchor,
    pub end: Anchor,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Anchor {
    /// if id is None, it means the anchor is at the beginning or the end of the document
    pub id: Option<OpID>,
    pub type_: AnchorType,
}

impl RangeOp {
    fn id(&self) -> OpID {
        match self {
            RangeOp::Patch(x) => x.id,
            RangeOp::Annotate(x) => x.id,
        }
    }

    #[allow(unused)]
    fn set_id(&mut self, id: OpID) {
        match self {
            RangeOp::Patch(x) => x.id = id,
            RangeOp::Annotate(x) => x.id = id,
        }
    }

    #[allow(unused)]
    fn lamport(&self) -> Lamport {
        match self {
            RangeOp::Patch(x) => x.lamport,
            RangeOp::Annotate(x) => x.range_lamport.0,
        }
    }
}

impl Anchor {
    pub fn before(id: OpID) -> Self {
        Self {
            id: Some(id),
            type_: AnchorType::Before,
        }
    }

    pub fn after(id: OpID) -> Self {
        Self {
            id: Some(id),
            type_: AnchorType::After,
        }
    }

    pub fn before_none() -> Self {
        Self {
            id: None,
            type_: AnchorType::Before,
        }
    }

    pub fn after_none() -> Self {
        Self {
            id: None,
            type_: AnchorType::After,
        }
    }
}

impl<T: RangeBounds<OpID>> From<T> for AnchorRange {
    fn from(range: T) -> Self {
        let start = match range.start_bound() {
            Bound::Included(x) => Anchor {
                id: Some(*x),
                type_: AnchorType::Before,
            },
            Bound::Excluded(x) => Anchor {
                id: Some(*x),
                type_: AnchorType::After,
            },
            Bound::Unbounded => Anchor {
                id: None,
                type_: AnchorType::After,
            },
        };
        let end = match range.end_bound() {
            Bound::Included(x) => Anchor {
                id: Some(*x),
                type_: AnchorType::After,
            },
            Bound::Excluded(x) => Anchor {
                id: Some(*x),
                type_: AnchorType::Before,
            },
            Bound::Unbounded => Anchor {
                id: None,
                type_: AnchorType::Before,
            },
        };
        Self { start, end }
    }
}

impl OpID {
    pub fn new(client: u64, counter: Counter) -> Self {
        Self { client, counter }
    }
}

#[derive(Debug)]
pub struct CrdtRange<R> {
    pub(crate) range_map: R,
}

impl<R: RangeMap + Debug> CrdtRange<R> {
    pub fn new() -> Self {
        let mut r = R::init();
        r.insert_directly(0, 2);
        CrdtRange { range_map: r }
    }

    /// Insert a new span of text into the range. It's used to sync
    /// List Crdt insert ops.  
    ///
    /// It will only generate new RangeOp(Patches) when inserting new
    /// text locally and there are annotations attached to the tombstones
    /// at `pos`.
    ///
    /// - `cmp(target)` returns whether the target is in right side or
    ///   left side of the new inserted op. `target` may be any op id
    ///   from the List CRDT because it's used to test both sides of an
    ///   annotation
    pub fn insert_text<Cmp>(
        &mut self,
        pos: usize,
        len: usize,
        is_local: bool,
        left_id: Option<OpID>,
        right_id: Option<OpID>,
        next_lamport: Lamport,
        next_op_id: OpID,
        mut cmp: Cmp,
    ) -> Vec<RangeOp>
    where
        Cmp: FnMut(OpID) -> Ordering,
    {
        let mut ans = vec![];
        // Maybe add the zero-len filter rule as a requirement for the range_map?
        let spans = self.get_trimmed_spans_around(pos);
        assert!(spans.len() <= 3, "{}", spans.len());
        assert!(spans.iter().map(|x| x.len).sum::<usize>() == 2);
        let non_empty_span_count = spans.iter().filter(|x| x.len != 0).count();
        if is_local && non_empty_span_count > 1 {
            self.gen_patch(
                non_empty_span_count,
                spans,
                left_id,
                right_id,
                next_lamport,
                next_op_id,
                &mut ans,
            );
        }

        self.range_map.insert(pos * 3 + 1, len * 3, |ann| {
            // dbg!(&tombstones, first_new_op_id, ann, relative);
            let start_before_insert = match ann.range.start.id {
                Some(id) => cmp(id) == Ordering::Less,
                None => true,
            };
            let end_after_insert = match ann.range.end.id {
                Some(id) => cmp(id) == Ordering::Greater,
                None => true,
            };
            match (start_before_insert, end_after_insert) {
                (true, true) => AnnPosRelativeToInsert::IncludeInsert,
                (true, false) => AnnPosRelativeToInsert::Before,
                (false, true) => AnnPosRelativeToInsert::After,
                (false, false) => unreachable!(),
            }
        });

        ans
    }

    fn get_trimmed_spans_around(&mut self, pos: usize) -> Vec<Span> {
        let mut spans: Vec<Span> = self
            .range_map
            .get_annotations(pos * 3, 2)
            .into_iter()
            .skip_while(|x| x.len == 0)
            .collect();
        for i in (0..spans.len()).rev() {
            if spans[i].len != 0 {
                spans.drain(i + 1..);
                break;
            }
        }
        spans
    }

    /// NOTE: This is error-prone, need more attention
    fn gen_patch(
        &mut self,
        non_empty_span_count: usize,
        spans: Vec<Span>,
        left_id: Option<OpID>,
        right_id: Option<OpID>,
        mut next_lamport: Lamport,
        mut next_op_id: OpID,
        ans: &mut Vec<RangeOp>,
    ) {
        assert!(non_empty_span_count <= 2);
        let mut visited_left = false;
        let mut pure_left = BTreeSet::new();
        let mut pure_middle = BTreeSet::new();
        let mut left_annotations = BTreeSet::new();
        let mut right_annotations = BTreeSet::new();
        for span in spans {
            if !visited_left {
                // left
                assert_eq!(span.len, 1);
                visited_left = true;
                pure_left = span.annotations.clone();
                left_annotations = span.annotations;
            } else if span.len == 0 {
                // middle
                pure_middle = span.annotations;
            } else {
                // right
                assert_eq!(span.len, 1);
                for ann in span.annotations.iter() {
                    right_annotations.insert(ann.clone());
                    left_annotations.remove(ann);
                    pure_middle.remove(ann);
                }
            }
        }

        for ann in pure_left {
            right_annotations.remove(&ann);
            pure_middle.remove(&ann);
        }

        for annotation in left_annotations {
            let end_id = annotation.range.end.id;
            if end_id != left_id && end_id != right_id {
                // TODO: simplify
                if AnchorType::Before == annotation.range.end.type_ {
                    ans.push(RangeOp::Patch(Patch {
                        id: next_op_id,
                        lamport: next_lamport,
                        target_range_id: annotation.id,
                        move_start_to: annotation.range.start.id,
                        move_end_to: right_id,
                    }));
                    self.range_map.adjust_annotation(
                        annotation.id,
                        next_lamport,
                        next_op_id,
                        None,
                        Some((1, right_id)),
                    );
                    next_op_id.counter += 1;
                    next_lamport += 1;
                } else if !pure_middle.contains(&annotation) {
                    ans.push(RangeOp::Patch(Patch {
                        id: next_op_id,
                        lamport: next_lamport,
                        target_range_id: annotation.id,
                        move_start_to: annotation.range.start.id,
                        move_end_to: left_id,
                    }));
                    self.range_map.adjust_annotation(
                        annotation.id,
                        next_lamport,
                        next_op_id,
                        None,
                        Some((-1, left_id)),
                    );
                    next_op_id.counter += 1;
                    next_lamport += 1;
                }
            }
        }

        for annotation in right_annotations {
            let start_id = annotation.range.start.id;
            if start_id != left_id && start_id != right_id {
                match annotation.range.start.type_ {
                    AnchorType::Before => {
                        if !pure_middle.contains(&annotation) {
                            ans.push(RangeOp::Patch(Patch {
                                id: next_op_id,
                                lamport: next_lamport,
                                target_range_id: annotation.id,
                                move_start_to: right_id,
                                move_end_to: annotation.range.end.id,
                            }));
                            self.range_map.adjust_annotation(
                                annotation.id,
                                next_lamport,
                                next_op_id,
                                Some((1, right_id)),
                                None,
                            );
                            next_op_id.counter += 1;
                            next_lamport += 1;
                        }
                    }
                    AnchorType::After => {
                        ans.push(RangeOp::Patch(Patch {
                            id: next_op_id,
                            lamport: next_lamport,
                            target_range_id: annotation.id,
                            move_start_to: right_id,
                            move_end_to: annotation.range.end.id,
                        }));
                        self.range_map.adjust_annotation(
                            annotation.id,
                            next_lamport,
                            next_op_id,
                            Some((-1, left_id)),
                            None,
                        );
                        next_op_id.counter += 1;
                        next_lamport += 1;
                    }
                }
            }
        }
    }

    /// NOTE: This is error-prone, need more attention
    fn apply_remote_patch<Index>(&mut self, patch: Patch, index: &Index)
    where
        Index: Fn(OpID) -> Result<usize, usize>,
    {
        let Some((ann, pos)) = self.range_map.get_annotation_pos(patch.target_range_id) else { return };
        let new_start = index_start(
            Anchor {
                id: patch.move_start_to,
                type_: ann.range.start.type_,
            },
            index,
        );
        let new_end = index_end(
            Anchor {
                id: patch.move_end_to,
                type_: ann.range.end.type_,
            },
            index,
        )
        .unwrap_or(self.range_map.len());

        self.range_map.adjust_annotation(
            patch.target_range_id,
            patch.lamport,
            patch.id,
            Some((new_start as isize - pos.start as isize, patch.move_start_to)),
            Some((new_end as isize - pos.end as isize, patch.move_end_to)),
        );
    }

    pub fn delete_text(&mut self, pos: usize, len: usize) {
        self.range_map.delete(pos * 3 + 1, len * 3);
    }

    pub fn annotate(&mut self, annotation: Annotation, range: impl RangeBounds<usize>) -> RangeOp {
        let start = match range.start_bound() {
            Bound::Included(x) => *x * 3 + 2,
            Bound::Excluded(x) => *x * 3 + 3,
            Bound::Unbounded => 0,
        };
        assert!(annotation.range.start.type_ != AnchorType::After);
        assert!(annotation.range.start.id.is_some());
        let end = match range.end_bound() {
            Bound::Included(x) => *x * 3 + 3,
            Bound::Excluded(x) => *x * 3 + 2,
            Bound::Unbounded => self.range_map.len(),
        };
        self.range_map
            .annotate(start, end - start, annotation.clone());
        RangeOp::Annotate(annotation)
    }

    pub fn delete_annotation(&mut self, lamport: Lamport, op_id: OpID, target_id: OpID) -> RangeOp {
        self.range_map.delete_annotation(target_id);
        RangeOp::Patch(Patch {
            id: op_id,
            target_range_id: target_id,
            move_start_to: None,
            move_end_to: None,
            lamport,
        })
    }

    pub fn apply_remote_op<Index>(&mut self, op: RangeOp, index: &Index)
    where
        Index: Fn(OpID) -> Result<usize, usize>,
    {
        match op {
            RangeOp::Patch(patch) => {
                self.apply_remote_patch(patch, index);
            }
            RangeOp::Annotate(a) => {
                let start = index_start(a.range.start, index);
                let end = index_end(a.range.end, index).unwrap_or(self.range_map.len());
                self.range_map.annotate(start, end - start, a)
            }
        }
    }

    pub fn get_annotation_range(&mut self, id: OpID) -> Option<Range<usize>> {
        let (_, range) = self.range_map.get_annotation_pos(id)?;
        Some((range.start / 3)..(range.end / 3))
    }

    pub fn get_annotations(&mut self, range: impl RangeBounds<usize>) -> Vec<Span> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => x * 3 + 2,
            std::ops::Bound::Excluded(_) => unreachable!(),
            std::ops::Bound::Unbounded => 2,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(x) => x * 3 + 3,
            std::ops::Bound::Excluded(x) => x * 3,
            std::ops::Bound::Unbounded => self.range_map.len(),
        };

        let mut last_index = 0;
        let mut ans = vec![];
        for mut span in self
            .range_map
            .get_annotations(start, end - start)
            .into_iter()
        {
            let next_index = last_index + span.len;
            let len = (next_index + 2) / 3 - (last_index + 2) / 3;
            span.len = len;

            type Key = (Lamport, OpID);
            let mut annotations: HashMap<InternalString, (Key, Vec<Arc<Annotation>>)> =
                HashMap::new();
            for a in std::mem::take(&mut span.annotations) {
                if let Some(x) = annotations.get_mut(&a.type_) {
                    if a.behavior == Behavior::Inclusive {
                        x.1.push(a);
                    } else if a.range_lamport > x.0 {
                        *x = (a.range_lamport, vec![a]);
                    }
                } else {
                    annotations.insert(a.type_.clone(), (a.range_lamport, vec![a]));
                }
            }
            span.annotations = annotations.into_values().flat_map(|x| x.1).collect();
            ans.push(span);
            last_index = next_index;
        }

        ans
    }

    pub fn len(&self) -> usize {
        self.range_map.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.range_map.len() == 2
    }
}

fn index_start<Index>(start: Anchor, index: &Index) -> usize
where
    Index: Fn(OpID) -> Result<usize, usize>,
{
    start
        .id
        .map(|x| match index(x) {
            Ok(x) => {
                if start.type_ == AnchorType::Before {
                    x * 3 + 2
                } else {
                    x * 3 + 3
                }
            }
            Err(x) => x * 3 + 1,
        })
        .unwrap_or(0)
}

fn index_end<Index>(end: Anchor, index: &Index) -> Option<usize>
where
    Index: Fn(OpID) -> Result<usize, usize>,
{
    end.id.map(|x| match index(x) {
        Ok(x) => {
            if end.type_ == AnchorType::Before {
                x * 3 + 2
            } else {
                x * 3 + 3
            }
        }

        Err(x) => x * 3 + 1,
    })
}

impl<R: RangeMap + Debug> Default for CrdtRange<R> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "test")]
pub mod test_utils;
