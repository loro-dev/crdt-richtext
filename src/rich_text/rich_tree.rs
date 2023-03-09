use crate::{
    range_map::{small_set::SmallSetI32, tree_impl::AnchorSet},
    Counter, Lamport, OpID,
};
use append_only_bytes::BytesSlice;
use core::fmt;
use fxhash::FxHashSet;
use generic_btree::rle::{HasLength, Mergeable, Sliceable};
use smallvec::SmallVec;
use std::{mem::take, str::Chars};

use self::{rich_tree_btree_impl::RichTreeTrait, utf16::get_utf16_len};

pub(crate) mod query;
pub(crate) mod rich_tree_btree_impl;
mod utf16;

type AnnIdx = i32;

#[derive(Debug, Clone)]
pub struct Elem {
    pub id: OpID,
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
            id,
            left,
            lamport,
            utf16_len: get_utf16_len(&string),
            string,
            status: Status::ALIVE,
            anchor_set: AnchorSet::default(),
        }
    }

    pub fn id_last(&self) -> OpID {
        OpID {
            client: self.id.client,
            counter: self.id.counter + self.atom_len() as Counter - 1,
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
            id: self.id.inc(start as Counter),
            left: Some(self.id.inc(start as Counter - 1)),
            lamport: self.lamport + start as Lamport,
            string: s,
            utf16_len,
            status: self.status,
        };
        self.utf16_len -= utf16_len;
        self.string = self.string.slice_clone(..offset);
        right
    }

    pub fn local_delete(&mut self) -> bool {
        if !self.is_dead() {
            self.status.deleted_times += 1;
            true
        } else {
            false
        }
    }

    pub fn apply_remote_delete(&mut self) {
        self.status.deleted_times += 1;
    }

    #[must_use]
    pub fn update(
        &mut self,
        start: usize,
        end: usize,
        f: &mut impl FnMut(&mut Elem),
    ) -> SmallVec<[Elem; 2]> {
        let mut ans = SmallVec::new();
        if start == end {
            return ans;
        }

        assert!(end > start);
        if start == 0 && end == self.atom_len() {
            f(self);
            return ans;
        }
        if start == 0 {
            let right = self.split(end);
            f(self);
            ans.push(right);
            return ans;
        }
        if end == self.atom_len() {
            let mut right = self.split(start);
            f(&mut right);
            ans.push(right);
            return ans;
        }

        let mut middle = self.split(start);
        let right = middle.split(end - start);
        f(&mut middle);
        ans.push(middle);
        ans.push(right);
        ans
    }

    pub fn merge_slice(&mut self, s: &BytesSlice) {
        self.string.try_merge(s).unwrap();
        self.utf16_len += get_utf16_len(s);
    }

    pub fn contains_id(&self, id: OpID) -> bool {
        id.client == self.id.client
            && self.id.counter <= id.counter
            && self.id.counter + self.rle_len() as Counter > id.counter
    }

    pub fn overlap(&self, id: OpID, len: usize) -> bool {
        id.client == self.id.client
            && self.id.counter < id.counter + len as Counter
            && self.id.counter + self.rle_len() as Counter > id.counter as Counter
    }
}

impl Mergeable for Elem {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.id.client == rhs.id.client
            && self.id.counter + self.atom_len() as Counter == rhs.id.counter
            && self.lamport + self.atom_len() as Lamport == rhs.lamport
            && rhs.left == Some(self.id)
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
        self.id = lhs.id;
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
            id: self.id.inc(start as Counter),
            left: if start == 0 {
                self.left
            } else {
                Some(self.id.inc(start as Counter - 1))
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
        self.id = self.id.inc(start as Counter);
        self.left = if start == 0 {
            self.left
        } else {
            Some(self.id.inc(start as Counter - 1))
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
    pub const ALIVE: Status = Status {
        future: false,
        deleted_times: 0,
    };
    fn new() -> Self {
        Status {
            future: false,
            deleted_times: 0,
        }
    }

    #[inline(always)]
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
