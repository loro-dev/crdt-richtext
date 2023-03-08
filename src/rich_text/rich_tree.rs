use crate::{
    range_map::{small_set::SmallSetI32, tree_impl::AnchorSet},
    Counter, Lamport, OpID,
};
use append_only_bytes::BytesSlice;
use core::fmt;
use fxhash::FxHashSet;
use generic_btree::rle::{HasLength, Mergeable, Sliceable};
use std::{mem::take, str::Chars};

use self::{rich_tree_btree_impl::RichTreeTrait, utf16::get_utf16_len};

pub(crate) mod query;
pub(crate) mod rich_tree_btree_impl;
mod utf16;

type AnnIdx = i32;

#[derive(Debug, Clone)]
pub struct Elem {
    pub start_id: OpID,
    pub left: Option<OpID>,
    pub lamport: Lamport,
    pub string: BytesSlice,
    pub utf16_len: usize,
    pub status: Status,
    pub anchor_set: AnchorSet,
}

impl Elem {
    pub fn new(id: OpID, left: Option<OpID>, lamport: Lamport, string: BytesSlice) -> Self {
        Elem {
            start_id: id,
            left,
            lamport,
            utf16_len: get_utf16_len(&string),
            string,
            status: Status::Alive,
            anchor_set: AnchorSet::default(),
        }
    }

    pub fn id_last(&self) -> OpID {
        OpID {
            client: self.start_id.client,
            counter: self.start_id.counter + self.atom_len() as Counter - 1,
        }
    }

    #[inline(always)]
    pub fn content_len(&self) -> usize {
        if self.status.is_dead() {
            0
        } else {
            self.string.len()
        }
    }

    #[inline(always)]
    pub fn atom_len(&self) -> usize {
        self.string.len()
    }

    #[inline(always)]
    pub fn is_dead(&self) -> bool {
        self.status.is_dead()
    }

    #[inline(always)]
    pub fn status(&self) -> Status {
        self.status
    }

    pub fn split(&mut self, offset: usize) -> Self {
        assert!(offset != 0);
        let start = offset;
        let s = self.string.slice_clone(offset..);
        let utf16_len = get_utf16_len(&s);
        let right = Self {
            anchor_set: AnchorSet {
                start: Default::default(),
                end: take(&mut self.anchor_set.end),
            },
            start_id: self.start_id.inc(start as Counter),
            left: Some(self.start_id.inc(start as Counter - 1)),
            lamport: self.lamport + start as Lamport,
            string: s,
            utf16_len,
            status: self.status,
        };
        self.utf16_len -= utf16_len;
        self.string = self.string.slice_clone(..offset);
        right
    }
}

impl Mergeable for Elem {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.start_id.client == rhs.start_id.client
            && self.start_id.counter + self.atom_len() as Counter == rhs.start_id.counter
            && self.lamport + self.atom_len() as Lamport == rhs.lamport
            && rhs.left == Some(self.start_id)
            && self.status == rhs.status
            && self.string.can_merge(&rhs.string)
            && self.anchor_set.end.is_empty()
            && rhs.anchor_set.start.is_empty()
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.string.try_merge(&rhs.string).unwrap();
        self.utf16_len += rhs.utf16_len;
        self.anchor_set.end = rhs.anchor_set.end.clone();
    }

    fn merge_left(&mut self, lhs: &Self) {
        self.start_id = lhs.start_id;
        self.left = lhs.left;
        self.lamport = lhs.lamport;
        let mut string = lhs.string.clone();
        string.try_merge(&self.string).unwrap();
        self.string = string;
        self.utf16_len += lhs.utf16_len;
        self.anchor_set.start = lhs.anchor_set.start.clone();
    }
}

impl HasLength for Elem {
    fn rle_len(&self) -> usize {
        self.atom_len()
    }
}

impl Sliceable for Elem {
    fn slice(&self, range: impl std::ops::RangeBounds<usize>) -> Self {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => *x + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(x) => *x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.atom_len(),
        };
        let s = self.string.slice_clone(range);
        let utf16_len = get_utf16_len(&s);
        Self {
            anchor_set: AnchorSet {
                start: if start == 0 {
                    self.anchor_set.start.clone()
                } else {
                    Default::default()
                },
                end: if end == self.atom_len() {
                    self.anchor_set.end.clone()
                } else {
                    Default::default()
                },
            },
            start_id: self.start_id.inc(start as Counter),
            left: if start == 0 {
                self.left
            } else {
                Some(self.start_id.inc(start as Counter - 1))
            },
            lamport: self.lamport + start as Lamport,
            string: s,
            utf16_len,
            status: self.status,
        }
    }

    fn slice_(&mut self, range: impl std::ops::RangeBounds<usize>)
    where
        Self: Sized,
    {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => *x + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(x) => *x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.atom_len(),
        };
        if start == 0 && end == self.atom_len() {
            return;
        }

        if start != 0 {
            self.anchor_set.start.clear();
        }
        if end != self.atom_len() {
            self.anchor_set.end.clear();
        }
        self.start_id = self.start_id.inc(start as Counter);
        self.left = if start == 0 {
            self.left
        } else {
            Some(self.start_id.inc(start as Counter - 1))
        };
        self.lamport += start as Lamport;
        self.string = self.string.slice_clone(range);
        self.utf16_len = get_utf16_len(&self.string);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub future: bool,
    pub deleted_times: u16,
}

impl Status {
    pub const Alive: Status = Status {
        future: false,
        deleted_times: 0,
    };
    fn new() -> Self {
        Status {
            future: false,
            deleted_times: 0,
        }
    }

    fn is_dead(&self) -> bool {
        self.future || self.deleted_times > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Cache {
    pub len: usize,
    pub utf16_len: usize,
    pub anchor_set: AnchorSet,
}

#[derive(Default, Debug)]
pub(crate) struct CacheDiff {
    start: SmallSetI32,
    end: SmallSetI32,
    len_diff: isize,
    utf16_len_diff: isize,
}

impl Cache {
    fn apply_diff(&mut self, diff: &CacheDiff) {
        self.len = (self.len as isize + diff.len_diff) as usize;
        self.utf16_len = (self.utf16_len as isize + diff.utf16_len_diff) as usize;
        for ann in diff.start.iter() {
            if ann >= 0 {
                self.anchor_set.start.insert(ann);
            } else {
                self.anchor_set.start.remove(&(-ann));
            }
        }
        for ann in diff.end.iter() {
            if ann >= 0 {
                self.anchor_set.end.insert(ann);
            } else {
                self.anchor_set.end.remove(&(-ann));
            }
        }
    }
}
