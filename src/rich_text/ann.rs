use std::{mem::take, sync::Arc};

use append_only_bytes::BytesSlice;
use fxhash::{FxHashMap, FxHashSet};
use generic_btree::rle::{HasLength, Mergeable};
use smallvec::SmallVec;

use crate::{range_map::small_set::SmallSetI32, AnchorType, Annotation, InternalString, OpID};

use super::rich_tree::{CacheDiff, Elem};

/// Use negative to represent deletions
pub type AnnIdx = i32;

#[derive(Default, Debug)]
pub struct AnnManager {
    idx_to_ann: Vec<Arc<Annotation>>,
    id_to_idx: FxHashMap<OpID, AnnIdx>,
}

impl AnnManager {
    #[inline(always)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, new: Arc<Annotation>) -> AnnIdx {
        if self.idx_to_ann.is_empty() {
            // We don't use the zero pos
            self.idx_to_ann.push(new.clone());
        }

        let id = new.id;
        let idx = self.idx_to_ann.len() as i32;
        self.idx_to_ann.push(new);
        self.id_to_idx.insert(id, idx);
        idx
    }

    #[inline(always)]
    pub fn get_ann_by_idx(&self, idx: AnnIdx) -> Option<&Arc<Annotation>> {
        self.idx_to_ann.get(idx as usize)
    }

    #[inline(always)]
    pub fn get_ann_by_id(&self, id: OpID) -> Option<&Arc<Annotation>> {
        let idx = self.id_to_idx.get(&id)?;
        self.idx_to_ann.get(*idx as usize)
    }

    #[inline(always)]
    pub fn get_idx_by_id(&self, id: OpID) -> Option<AnnIdx> {
        self.id_to_idx.get(&id).copied()
    }
}

/// The annotated text span.
#[derive(Debug, Clone)]
pub struct Span {
    pub text: BytesSlice,
    pub annotations: FxHashSet<InternalString>,
}

impl Span {
    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(self.text.as_ref()).unwrap()
    }
}

pub fn apply_start_ann_set(set: &mut FxHashSet<AnnIdx>, start: &FxHashSet<AnnIdx>) {
    for elem in start.iter() {
        set.insert(*elem);
    }
}

pub fn apply_end_ann_set(set: &mut FxHashSet<AnnIdx>, end: &FxHashSet<AnnIdx>) {
    for elem in end.iter() {
        set.remove(elem);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheAnchorSet {
    start: FxHashSet<AnnIdx>,
    end: FxHashSet<AnnIdx>,
}

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct ElemAnchorSet {
    start_at_start: FxHashSet<AnnIdx>,
    end_at_start: FxHashSet<AnnIdx>,
    start_at_end: FxHashSet<AnnIdx>,
    end_at_end: FxHashSet<AnnIdx>,
}

impl Mergeable for ElemAnchorSet {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.start_at_end.is_empty()
            && self.end_at_end.is_empty()
            && rhs.start_at_start.is_empty()
            && rhs.end_at_start.is_empty()
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.start_at_end = rhs.start_at_end.clone();
        self.end_at_end = rhs.end_at_end.clone();
    }

    fn merge_left(&mut self, left: &Self) {
        self.start_at_start = left.start_at_start.clone();
        self.end_at_start = left.end_at_start.clone();
    }
}

impl CacheAnchorSet {
    pub fn calc_diff(&self, other: &Self) -> AnchorSetDiff {
        let mut ans: AnchorSetDiff = Default::default();
        for ann in self.start.difference(&other.start) {
            ans.start.insert(*ann);
        }
        for ann in other.start.difference(&self.start) {
            ans.start.insert(-*ann);
        }
        for ann in self.end.difference(&other.end) {
            ans.end.insert(*ann);
        }
        for ann in other.end.difference(&self.end) {
            ans.end.insert(-*ann);
        }

        ans
    }

    pub fn apply_diff(&mut self, diff: &AnchorSetDiff) {
        for ann in diff.start.iter() {
            if ann > 0 {
                self.start.insert(ann);
            } else {
                self.start.remove(&(-ann));
            }
        }
        for ann in diff.end.iter() {
            if ann > 0 {
                self.end.insert(ann);
            } else {
                self.end.remove(&(-ann));
            }
        }
    }

    #[inline]
    pub fn contains_start(&self, ann: AnnIdx) -> bool {
        self.start.contains(&ann)
    }

    #[inline]
    pub fn contains_end(&self, ann: AnnIdx) -> bool {
        self.end.contains(&ann)
    }

    pub fn union_(&mut self, other: &Self) {
        self.start.extend(other.start.iter());
        self.end.extend(other.end.iter());
    }

    pub fn union_elem_set(&mut self, other: &ElemAnchorSet) {
        self.start.extend(other.start_at_start.iter());
        self.start.extend(other.start_at_end.iter());
        self.end.extend(other.end_at_start.iter());
        self.end.extend(other.end_at_end.iter());
    }
}

impl ElemAnchorSet {
    pub fn contains_start(&self, ann: AnnIdx) -> (bool, bool) {
        let a = self.start_at_start.contains(&ann);
        let b = self.start_at_end.contains(&ann);
        (a || b, a)
    }

    /// return (contains_end, is_inclusive)
    pub fn contains_end(&self, ann: AnnIdx) -> (bool, bool) {
        let a = self.end_at_start.contains(&ann);
        let b = self.end_at_end.contains(&ann);
        (a || b, b)
    }

    pub fn calc_diff(&self, other: &Self) -> AnchorSetDiff {
        let mut ans: AnchorSetDiff = Default::default();
        for ann in self.start_at_start.difference(&other.start_at_start) {
            ans.start.insert(*ann);
        }
        for ann in other.start_at_start.difference(&self.start_at_start) {
            ans.start.insert(-*ann);
        }
        for ann in self.end_at_start.difference(&other.end_at_start) {
            ans.end.insert(*ann);
        }
        for ann in other.end_at_start.difference(&self.end_at_start) {
            ans.end.insert(-*ann);
        }
        for ann in self.start_at_end.difference(&other.start_at_end) {
            ans.start.insert(*ann);
        }
        for ann in other.start_at_end.difference(&self.start_at_end) {
            ans.start.insert(-*ann);
        }
        for ann in self.end_at_end.difference(&other.end_at_end) {
            ans.end.insert(*ann);
        }
        for ann in other.end_at_end.difference(&self.end_at_end) {
            ans.end.insert(-*ann);
        }

        ans
    }

    #[inline]
    pub fn insert_ann_start(&mut self, idx: AnnIdx, type_: AnchorType) {
        match type_ {
            AnchorType::Before => self.start_at_start.insert(idx),
            AnchorType::After => self.start_at_end.insert(idx),
        };
    }

    #[inline]
    pub fn insert_ann_end(&mut self, idx: AnnIdx, type_: AnchorType) {
        match type_ {
            AnchorType::Before => self.end_at_start.insert(idx),
            AnchorType::After => self.end_at_end.insert(idx),
        };
    }

    pub fn insert_ann(&mut self, idx: AnnIdx, type_: AnchorType, is_start: bool) {
        if is_start {
            match type_ {
                AnchorType::Before => self.start_at_start.insert(idx),
                AnchorType::After => self.start_at_end.insert(idx),
            };
        } else {
            match type_ {
                AnchorType::Before => self.end_at_start.insert(idx),
                AnchorType::After => self.end_at_end.insert(idx),
            };
        }
    }

    #[inline]
    pub fn insert_start_at_start(&mut self, idx: AnnIdx) {
        self.start_at_start.insert(idx);
    }

    #[inline]
    pub fn insert_start_at_end(&mut self, idx: AnnIdx) {
        self.start_at_end.insert(idx);
    }

    #[inline]
    pub fn insert_end_at_start(&mut self, idx: AnnIdx) {
        self.end_at_start.insert(idx);
    }

    #[inline]
    pub fn insert_end_at_end(&mut self, idx: AnnIdx) {
        self.end_at_end.insert(idx);
    }

    pub(crate) fn split(&mut self) -> ElemAnchorSet {
        ElemAnchorSet {
            start_at_start: Default::default(),
            end_at_start: Default::default(),
            start_at_end: take(&mut self.start_at_end),
            end_at_end: take(&mut self.end_at_end),
        }
    }

    pub(crate) fn trim(&self, trim_start: bool, trim_end: bool) -> ElemAnchorSet {
        let mut ans = ElemAnchorSet::default();
        if !trim_start {
            ans.start_at_start = self.start_at_start.clone();
            ans.end_at_start = self.end_at_start.clone();
        }
        if !trim_end {
            ans.start_at_end = self.start_at_end.clone();
            ans.end_at_end = self.end_at_end.clone();
        }
        ans
    }

    pub(crate) fn trim_(&mut self, trim_start: bool, trim_end: bool) {
        if trim_start {
            self.start_at_start.clear();
            self.end_at_start.clear();
        }
        if trim_end {
            self.start_at_end.clear();
            self.end_at_end.clear();
        }
    }

    pub fn cache_anchor_set(&self) -> CacheAnchorSet {
        let mut ans = CacheAnchorSet::default();
        if !self.start_at_start.is_empty() {
            for ann in self.start_at_start.iter() {
                ans.start.insert(*ann);
            }
        }

        if !self.end_at_start.is_empty() {
            for ann in self.end_at_start.iter() {
                ans.end.insert(*ann);
            }
        }

        if !self.start_at_end.is_empty() {
            for ann in self.start_at_end.iter() {
                ans.start.insert(*ann);
            }
        }

        if !self.end_at_end.is_empty() {
            for ann in self.end_at_end.iter() {
                ans.end.insert(*ann);
            }
        }

        ans
    }
}

#[derive(Debug, Default)]
pub struct AnchorSetDiff {
    start: SmallSetI32,
    end: SmallSetI32,
}

impl AnchorSetDiff {
    pub fn merge(&mut self, other: &Self) {
        for ann in other.start.iter() {
            self.start.insert(ann);
        }
        for ann in other.end.iter() {
            self.end.insert(ann);
        }
    }

    pub fn insert(&mut self, ann: AnnIdx, is_start: bool) {
        if is_start {
            self.start.insert(ann);
        } else {
            self.end.insert(ann);
        }
    }
}

impl From<AnchorSetDiff> for CacheDiff {
    fn from(value: AnchorSetDiff) -> Self {
        Self {
            anchor_diff: value,
            len_diff: 0,
            utf16_len_diff: 0,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct StyleCalculator(FxHashSet<AnnIdx>);

impl StyleCalculator {
    pub fn insert_start(&mut self, start: AnnIdx) {
        self.0.insert(start);
    }

    pub fn apply_node_start(&mut self, anchor_set: &CacheAnchorSet) {
        for ann in anchor_set.start.iter() {
            self.0.insert(*ann);
        }
    }

    pub fn apply_node_end(&mut self, anchor_set: &CacheAnchorSet) {
        for ann in anchor_set.end.iter() {
            self.0.remove(ann);
        }
    }

    pub fn apply_start(&mut self, anchor_set: &ElemAnchorSet) {
        for ann in anchor_set.start_at_start.iter() {
            self.0.insert(*ann);
        }
        for ann in anchor_set.end_at_start.iter() {
            self.0.remove(ann);
        }
    }

    pub fn apply_end(&mut self, anchor_set: &ElemAnchorSet) {
        for ann in anchor_set.start_at_end.iter() {
            self.0.insert(*ann);
        }
        for ann in anchor_set.end_at_end.iter() {
            self.0.remove(ann);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &AnnIdx> {
        self.0.iter()
    }
}

pub fn insert_anchor(
    elements: &mut Vec<Elem>,
    index: usize,
    offset: usize,
    ann: AnnIdx,
    type_: AnchorType,
    is_start: bool,
) -> AnchorSetDiff {
    match type_ {
        AnchorType::Before => {
            debug_assert!(offset < elements[index].rle_len());
            if offset == 0 {
                elements[index].anchor_set.insert_ann(ann, type_, is_start);
            } else {
                let new_elem = elements[index].split(offset);
                elements[index].anchor_set.insert_ann(ann, type_, is_start);
                elements.insert(index + 1, new_elem);
            }
        }
        AnchorType::After => {
            debug_assert!(offset < elements[index].rle_len());
            if offset == elements[index].rle_len() - 1 {
                elements[index].anchor_set.insert_ann(ann, type_, is_start);
            } else {
                let new_elem = elements[index].split(offset + 1);
                elements[index].anchor_set.insert_ann(ann, type_, is_start);
                elements.insert(index + 1, new_elem);
            }
        }
    }

    let mut diff = AnchorSetDiff::default();
    diff.insert(ann, is_start);
    diff
}

pub fn insert_anchors_at_same_elem(
    elem: &mut Elem,
    start_offset: usize,
    end_offset: usize,
    ann: AnnIdx,
    start_type: AnchorType,
    end_type: AnchorType,
) -> SmallVec<[Elem; 2]> {
    let mut ans = SmallVec::new();
    match (start_type, end_type) {
        (AnchorType::Before, AnchorType::Before) => {
            if start_offset == 0 {
                let mut new_elem = elem.split(end_offset);
                elem.anchor_set.insert_ann(ann, AnchorType::Before, true);
                new_elem
                    .anchor_set
                    .insert_ann(ann, AnchorType::Before, false);
                ans.push(new_elem);
            } else {
                for v in elem.update_twice(
                    start_offset,
                    end_offset,
                    elem.rle_len(),
                    &mut |elem| {
                        elem.anchor_set.insert_ann(ann, AnchorType::Before, true);
                    },
                    &mut |elem| {
                        elem.anchor_set.insert_ann(ann, AnchorType::Before, false);
                    },
                ) {
                    ans.push(v);
                }
            }
        }
        (AnchorType::Before, AnchorType::After) => {
            ans = elem
                .update(start_offset, end_offset, &mut |elem| {
                    elem.anchor_set.insert_ann(ann, AnchorType::Before, true);
                })
                .0;
        }
        (AnchorType::After, AnchorType::Before) => {
            debug_assert!(start_offset + 1 <= end_offset);
            let mut middle = elem.split(start_offset + 1);
            elem.anchor_set.insert_ann(ann, AnchorType::After, true);
            let len = middle.rle_len();
            let (mut new, _) = middle.update(end_offset - start_offset, len, &mut |elem| {
                elem.anchor_set.insert_ann(ann, end_type, false);
            });
            ans.push(middle);
            ans.append(&mut new);
        }
        (AnchorType::After, AnchorType::After) => {
            debug_assert!(start_offset + 1 <= end_offset);
            let mut middle = elem.split(start_offset + 1);
            elem.anchor_set.insert_ann(ann, AnchorType::After, true);
            let (mut new, _) = middle.update(0, end_offset + 1 - start_offset, &mut |elem| {
                elem.anchor_set.insert_ann(ann, end_type, false);
            });
            ans.push(middle);
            ans.append(&mut new);
        }
    }

    ans
}
