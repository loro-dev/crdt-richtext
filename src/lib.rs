use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashMap},
    fmt::Debug,
    ops::{Bound, RangeBounds},
    sync::Arc,
};

use range_map::{AnnPosRelativeToInsert, RangeMap, Span};

mod range_map;
type Lamport = u32;
type ClientID = u64;
type Counter = u32;

// TODO: make it generic?
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpID {
    client: ClientID,
    counter: Counter,
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
pub enum RangeMergeRule {
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
    pub move_start: bool,
    pub move_end: bool,
    pub move_start_to: Option<OpID>,
    pub move_end_to: Option<OpID>,
    pub lamport: Lamport,
}

#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub struct Annotation {
    pub id: OpID,
    pub lamport: Lamport,
    pub range: AnchorRange,
    pub merge_method: RangeMergeRule,
    // TODO: use internal string
    /// "bold", "comment", "italic", etc.
    pub type_: String,
    pub meta: Option<Vec<u8>>,
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

    fn set_id(&mut self, id: OpID) {
        match self {
            RangeOp::Patch(x) => x.id = id,
            RangeOp::Annotate(x) => x.id = id,
        }
    }

    fn lamport(&self) -> Lamport {
        match self {
            RangeOp::Patch(x) => x.lamport,
            RangeOp::Annotate(x) => x.lamport,
        }
    }

    fn set_lamport(&mut self, lamport: Lamport) {
        match self {
            RangeOp::Patch(x) => x.lamport = lamport,
            RangeOp::Annotate(x) => x.lamport = lamport,
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
    pub fn new(client: ClientID, counter: Counter) -> Self {
        Self { client, counter }
    }
}

#[derive(Debug)]
pub struct CrdtRange<R> {
    range_map: R,
}

impl<R: RangeMap + Debug> CrdtRange<R> {
    pub fn new() -> Self {
        let mut r = R::init();
        r.insert_directly(0, 2);
        CrdtRange { range_map: r }
    }

    /// `cmp` compare the positions of the given op and insert_op
    ///
    /// It may generate Patch only when is_local=true
    ///
    /// TODO: get next_id and lamport
    pub fn insert_text<Cmp>(
        &mut self,
        pos: usize,
        len: usize,
        is_local: bool,
        left_id: Option<OpID>,
        right_id: Option<OpID>,
        mut cmp: Cmp,
    ) -> Vec<RangeOp>
    where
        Cmp: FnMut(OpID) -> Ordering,
    {
        let mut ans = vec![];
        let spans = self.range_map.get_annotations(pos * 3, 2);
        assert!(spans.len() <= 3);
        assert!(spans.iter().map(|x| x.len).sum::<usize>() == 2);
        let non_empty_span_count = spans.iter().filter(|x| x.len != 0).count();
        if is_local && non_empty_span_count > 1 {
            assert!(non_empty_span_count <= 2);
            debug_log::group!("LOCAL HANDLER");
            debug_log::debug_dbg!("Before", &self.range_map);
            let mut visited_left = false;
            let mut pure_left = BTreeSet::new();
            let mut pure_middle = BTreeSet::new();
            let mut left_or_middle_annotations = BTreeSet::new();
            let mut right_or_middle_annotations = BTreeSet::new();
            for span in spans {
                if !visited_left {
                    // left
                    assert_eq!(span.len, 1);
                    visited_left = true;
                    pure_left = span.annotations.clone();
                    left_or_middle_annotations = span.annotations;
                } else if span.len == 0 {
                    // middle
                    for ann in span.annotations.iter() {
                        left_or_middle_annotations.insert(ann.clone());
                        right_or_middle_annotations.insert(ann.clone());
                    }
                    pure_middle = span.annotations;
                } else {
                    // right
                    assert_eq!(span.len, 1);
                    for ann in span.annotations.iter() {
                        right_or_middle_annotations.insert(ann.clone());
                        left_or_middle_annotations.remove(ann);
                        pure_middle.remove(ann);
                    }
                }
            }

            for ann in pure_left {
                right_or_middle_annotations.remove(&ann);
                pure_middle.remove(&ann);
            }

            for annotation in left_or_middle_annotations {
                let end_id = annotation.range.end.id;
                if end_id != left_id && end_id != right_id {
                    // TODO: simplify
                    if AnchorType::Before == annotation.range.end.type_ {
                        ans.push(RangeOp::Patch(Patch {
                            id: OpID {
                                client: 0,
                                counter: 0,
                            },
                            lamport: 0,
                            target_range_id: annotation.id,
                            move_start: false,
                            move_end: true,
                            move_start_to: None,
                            move_end_to: right_id,
                        }));
                        self.range_map
                            .adjust_annotation(annotation.id, None, Some((1, right_id)));
                    } else if !pure_middle.contains(&annotation) {
                        ans.push(RangeOp::Patch(Patch {
                            id: OpID {
                                client: 0,
                                counter: 0,
                            },
                            lamport: 0,
                            target_range_id: annotation.id,
                            move_start: false,
                            move_end: true,
                            move_start_to: None,
                            move_end_to: left_id,
                        }));
                        self.range_map
                            .adjust_annotation(annotation.id, None, Some((-1, left_id)));
                    }
                }
            }

            for annotation in right_or_middle_annotations {
                let start_id = annotation.range.start.id;
                if start_id != left_id && start_id != right_id {
                    match annotation.range.start.type_ {
                        AnchorType::Before => {
                            if !pure_middle.contains(&annotation) {
                                ans.push(RangeOp::Patch(Patch {
                                    id: OpID {
                                        client: 0,
                                        counter: 0,
                                    },
                                    lamport: 0,
                                    target_range_id: annotation.id,
                                    move_start: true,
                                    move_end: false,
                                    move_start_to: right_id,
                                    move_end_to: None,
                                }));
                                self.range_map.adjust_annotation(
                                    annotation.id,
                                    Some((1, right_id)),
                                    None,
                                );
                            }
                        }
                        AnchorType::After => {
                            ans.push(RangeOp::Patch(Patch {
                                id: OpID {
                                    client: 0,
                                    counter: 0,
                                },
                                lamport: 0,
                                target_range_id: annotation.id,
                                move_start: true,
                                move_end: false,
                                move_start_to: right_id,
                                move_end_to: None,
                            }));
                            self.range_map.adjust_annotation(
                                annotation.id,
                                Some((-1, left_id)),
                                None,
                            );
                        }
                    }
                }
            }
            debug_log::debug_dbg!("After", &self.range_map);
            debug_log::group_end!();
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
                (true, false) => AnnPosRelativeToInsert::BeforeInsert,
                (false, true) => AnnPosRelativeToInsert::AfterInsert,
                (false, false) => unreachable!(),
            }
        });

        ans
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
            move_end: true,
            move_start: true,
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
                if patch.move_start {
                    debug_log::debug_dbg!(&self.range_map, &patch,);
                    let (ann, pos) = self
                        .range_map
                        .get_annotation_pos(patch.target_range_id)
                        .unwrap();
                    let new_start = index_start(
                        Anchor {
                            id: patch.move_start_to,
                            type_: ann.range.start.type_,
                        },
                        index,
                    );
                    self.range_map.adjust_annotation(
                        patch.target_range_id,
                        Some((new_start as isize - pos.start as isize, patch.move_start_to)),
                        None,
                    );
                }
                if patch.move_end {
                    let (ann, pos) = self
                        .range_map
                        .get_annotation_pos(patch.target_range_id)
                        .unwrap();
                    let new_end = index_end(
                        Anchor {
                            id: patch.move_end_to,
                            type_: ann.range.end.type_,
                        },
                        index,
                    )
                    .unwrap_or(self.range_map.len());
                    debug_log::debug_dbg!(&patch, &pos, &ann, self.range_map.len(), new_end);
                    self.range_map.adjust_annotation(
                        patch.target_range_id,
                        None,
                        Some((new_end as isize - pos.end as isize, patch.move_end_to)),
                    );
                }
            }
            RangeOp::Annotate(a) => {
                let start = index_start(a.range.start, index);
                let end = index_end(a.range.end, index).unwrap_or(self.range_map.len());
                self.range_map.annotate(start, end - start, a)
            }
        }
    }

    pub fn get_annotations(&self, range: impl RangeBounds<usize>) -> Vec<Span> {
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

            let mut annotations: HashMap<String, (Lamport, Vec<Arc<Annotation>>)> = HashMap::new();
            for a in std::mem::take(&mut span.annotations) {
                if let Some(x) = annotations.get_mut(&a.type_) {
                    if a.merge_method == RangeMergeRule::Inclusive {
                        x.1.push(a);
                    } else if a.lamport > x.0 {
                        *x = (a.lamport, vec![a]);
                    }
                } else {
                    annotations.insert(a.type_.clone(), (a.lamport, vec![a]));
                }
            }
            span.annotations = annotations.into_values().flat_map(|x| x.1).collect();
            ans.push(span);
            last_index = next_index;
        }

        ans
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
