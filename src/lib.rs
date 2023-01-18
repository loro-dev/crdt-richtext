use std::cmp::Ordering;

use enum_as_inner::EnumAsInner;

pub struct RangeOp {
    pub id: OpID,
}

#[derive(EnumAsInner)]
pub enum RangeOpType {
    Patch(Patch),
    Add(InsertRange),
    Del(RemoveRange),
}

pub struct Anchor {
    pub id: OpID,
    pub type_: AnchorType,
}

pub enum AnchorType {
    Before,
    After,
}

pub enum RangeMergeRule {
    /// When calculating the final state, it will keep all the ranges even if they have the same type
    ///
    /// For example, we would like to keep both comments alive even if they have overlapped regions
    Inclusive,
    /// When calculating the final state, it will merge the ranges that have overlapped regions and have the same type
    ///
    /// For example, [bold 2~5] can be merged with [bold 1~4] to produce [bold 1-5]
    Merge,
}

pub struct Patch {
    pub target_range_id: OpID,
    pub move_end_to: Option<OpID>,
    pub move_start_to: Option<OpID>,
}

pub struct InsertRange {
    pub start: Anchor,
    pub end: Anchor,
    pub merge_method: RangeMergeRule,
    // TODO: use internal string
    pub type_: String,
    pub meta: Option<Vec<u8>>,
}

#[derive(EnumAsInner)]
pub enum RemoveRange {
    RemoveRange {
        start: Anchor,
        end: Anchor,
        // TODO: use internal string
        type_: String,
    },
    RemoveById {
        id: OpID,
    },
}

type ClientID = u64;
type Counter = u32;

// TODO: make it generic?
pub struct OpID {
    client: ClientID,
    counter: Counter,
}

pub struct CrdtRange {}

impl CrdtRange {
    pub fn new() -> Self {
        CrdtRange {}
    }

    /// `get_ops_at_pos(anchors)` returns the list of ops at `pos`.
    ///
    /// - The first returned element is the alive element at the pos
    /// - The rest of the elements are tombstones at the position filtered by `anchors`
    pub fn insert_text<F>(&mut self, pos: usize, len: usize, get_ops_at_pos: F) -> Vec<Patch>
    where
        F: FnOnce(&[OpID]) -> (OpID, Vec<OpID>),
    {
        todo!()
    }

    pub fn delete_text(&mut self, pos: usize, len: usize) {
        todo!()
    }

    pub fn insert_range<Index>(&mut self, range: InsertRange, id: OpID, index: &Index) -> RangeOp
    where
        Index: Fn(OpID) -> usize,
    {
        todo!()
    }

    pub fn delete_range<Index>(&mut self, target: RemoveRange, index: &Index) -> RangeOp
    where
        Index: Fn(OpID) -> usize,
    {
        todo!()
    }

    pub fn apply(&mut self, op: RangeOp) {
        todo!()
    }
}
