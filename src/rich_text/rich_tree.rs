use crate::{Counter, OpID};
use append_only_bytes::BytesSlice;
use core::fmt;

use generic_btree::rle::{HasLength, Mergeable, Sliceable};
use smallvec::SmallVec;
use std::str::Chars;

use self::{rich_tree_btree_impl::RichTreeTrait, utf16::get_utf16_len};

use super::ann::{AnchorSetDiff, CacheAnchorSet, ElemAnchorSet};

pub(crate) mod query;
pub(crate) mod rich_tree_btree_impl;
pub mod utf16;

type AnnIdx = i32;

#[derive(Clone)]
pub struct Elem {
    pub id: OpID,
    pub left: Option<OpID>,
    pub right: Option<OpID>,
    pub string: BytesSlice,
    pub utf16_len: u32,
    pub status: Status,
    pub anchor_set: Box<ElemAnchorSet>,
}

#[test]
fn size() {
    assert_eq!(std::mem::size_of::<Elem>(), 96);
}

impl std::fmt::Debug for Elem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Elem")
            .field("id", &self.id)
            .field("left", &self.left)
            .field("right", &self.right)
            .field("string", &std::str::from_utf8(&self.string))
            .field("utf16_len", &self.utf16_len)
            .field("status", &self.status)
            .field("anchor_set", &self.anchor_set)
            .finish()
    }
}

impl Elem {
    pub fn new(id: OpID, left: Option<OpID>, right: Option<OpID>, string: BytesSlice) -> Self {
        Elem {
            id,
            left,
            right,
            utf16_len: get_utf16_len(&string),
            string,
            status: Status::ALIVE,
            anchor_set: Default::default(),
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

    pub fn split(&mut self, offset: usize) -> Self {
        assert!(offset != 0);
        let start = offset;
        let s = self.string.slice_clone(offset..);
        let utf16_len = get_utf16_len(&s);
        let right = Self {
            anchor_set: Box::new(self.anchor_set.split()),
            id: self.id.inc(start as Counter),
            left: Some(self.id.inc(start as Counter - 1)),
            right: self.right,
            string: s,
            utf16_len,
            status: self.status,
        };
        self.utf16_len -= utf16_len;
        self.string = self.string.slice_clone(..offset);
        right
    }

    #[inline(always)]
    pub fn local_delete(&mut self) -> bool {
        if !self.is_dead() {
            self.status.deleted_times += 1;
            true
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn apply_remote_delete(&mut self) {
        self.status.deleted_times += 1;
    }

    #[must_use]
    pub fn update<R>(
        &mut self,
        start: usize,
        end: usize,
        f: &mut dyn FnMut(&mut Elem) -> R,
    ) -> (SmallVec<[Elem; 2]>, Option<R>) {
        let mut ans = SmallVec::new();
        debug_assert!(start <= end && end <= self.rle_len());
        if start == end {
            return (ans, None);
        }

        assert!(end > start);
        if start == 0 && end == self.atom_len() {
            let r = f(self);
            return (ans, Some(r));
        }
        if start == 0 {
            let right = self.split(end);
            let r = f(self);
            ans.push(right);
            return (ans, Some(r));
        }
        if end == self.atom_len() {
            let mut right = self.split(start);
            let r = f(&mut right);
            ans.push(right);
            return (ans, Some(r));
        }

        let mut middle = self.split(start);
        let right = middle.split(end - start);
        let r = f(&mut middle);
        ans.push(middle);
        ans.push(right);
        (ans, Some(r))
    }

    #[must_use]
    pub fn update_twice(
        &mut self,
        f_start: usize,
        f_end_g_start: usize,
        g_end: usize,
        f: &mut dyn FnMut(&mut Elem),
        g: &mut dyn FnMut(&mut Elem),
    ) -> SmallVec<[Elem; 4]> {
        let mut ans = SmallVec::new();
        debug_assert!(f_start < f_end_g_start && f_end_g_start < g_end);
        debug_assert!(g_end <= self.rle_len());
        if f_start == 0 && g_end == self.atom_len() {
            let new = self.split(f_end_g_start);
            ans.push(new);
            f(self);
            g(&mut ans[0]);
            return ans;
        }

        if f_start == 0 {
            let mut middle = self.split(f_end_g_start);
            let mut new_elems = middle.update(0, g_end - f_end_g_start, g);
            ans.push(middle);
            ans.append(&mut new_elems.0);
            f(self);
            return ans;
        }

        if g_end == self.atom_len() {
            let mut middle = self.split(f_start);
            let mut new_elems = middle.update(0, f_end_g_start - f_start, f);
            ans.push(middle);
            ans.append(&mut new_elems.0);
            g(ans.last_mut().unwrap());
            return ans;
        }

        let len = self.atom_len();
        let mut left = self.split(f_start);
        let mut middle0 = left.split(f_end_g_start - f_start);
        let mut middle1 = middle0.split(g_end - f_end_g_start);
        let right = middle1.split(len - g_end);
        f(&mut middle0);
        g(&mut middle1);
        ans.push(left);
        ans.push(middle0);
        ans.push(middle1);
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

    pub fn try_merge_arr(arr: &mut Vec<Self>, mut from: usize, mut len: usize) -> bool {
        len = len.min(arr.len() - from);
        while len > 0 {
            let mut j = from + 1;
            while j < arr.len() {
                let (left, right) = arr.split_at_mut(j);
                if left[from].can_merge(&right[0]) {
                    left[from].merge_right(&right[0]);
                    j += 1;
                } else {
                    break;
                }
            }
            if j > from + 1 {
                arr.drain(from + 1..j);
                // may continue?
                len = len.saturating_sub(j - from);
                from += 1;
            } else {
                len -= 1;
                from += 1;
            }
        }

        false
    }

    #[inline]
    pub fn has_after_anchor(&self) -> bool {
        self.anchor_set.has_after_anchor()
    }

    #[inline]
    #[allow(unused)]
    pub fn has_before_anchor(&self) -> bool {
        self.anchor_set.has_before_anchor()
    }
}

impl Mergeable for Elem {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.id.client == rhs.id.client
            && self.id.counter + self.atom_len() as Counter == rhs.id.counter
            && rhs.left == Some(self.id_last())
            && self.right == rhs.right
            && self.status == rhs.status
            && self.string.can_merge(&rhs.string)
            && self.anchor_set.can_merge(&rhs.anchor_set)
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.string.try_merge(&rhs.string).unwrap();
        self.utf16_len += rhs.utf16_len;
        self.anchor_set.merge_right(&rhs.anchor_set);
    }

    fn merge_left(&mut self, lhs: &Self) {
        self.id = lhs.id;
        self.left = lhs.left;
        let mut string = lhs.string.clone();
        string.try_merge(&self.string).unwrap();
        self.string = string;
        self.utf16_len += lhs.utf16_len;
        self.anchor_set.merge_left(&lhs.anchor_set);
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
            anchor_set: Box::new(self.anchor_set.trim(start != 0, end != self.rle_len())),
            id: self.id.inc(start as Counter),
            left: if start == 0 {
                self.left
            } else {
                Some(self.id.inc(start as Counter - 1))
            },
            right: self.right,
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

        self.anchor_set.trim_(start != 0, end != self.atom_len());
        self.id = self.id.inc(start as Counter);
        self.left = if start == 0 {
            self.left
        } else {
            Some(self.id.inc(start as Counter - 1))
        };
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

    #[allow(unused)]
    pub fn new() -> Self {
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
    pub len: u32,
    pub utf16_len: u32,
    pub anchor_set: CacheAnchorSet,
}

#[derive(Default, Debug)]
pub(crate) struct CacheDiff {
    pub(super) anchor_diff: AnchorSetDiff,
    pub(super) len_diff: isize,
    pub(super) utf16_len_diff: isize,
}

impl Cache {
    fn apply_diff(&mut self, diff: &CacheDiff) {
        self.len = (self.len as isize + diff.len_diff) as u32;
        self.utf16_len = (self.utf16_len as isize + diff.utf16_len_diff) as u32;
        self.anchor_set.apply_diff(&diff.anchor_diff);
    }
}

impl CacheDiff {
    pub fn new_len_diff(diff: isize, utf16_len_diff: isize) -> CacheDiff {
        CacheDiff {
            len_diff: diff,
            utf16_len_diff,
            anchor_diff: Default::default(),
        }
    }
}
