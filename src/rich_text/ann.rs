use fxhash::{FxHashMap, FxHashSet};
use generic_btree::rle::{HasLength, Mergeable};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use smallvec::SmallVec;
use std::{mem::take, sync::Arc};

use crate::{small_set::SmallSetI32, AnchorType, Annotation, Behavior, InternalString, OpID};

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

    #[allow(unused)]
    #[inline(always)]
    pub fn get_ann_by_id(&self, id: OpID) -> Option<&Arc<Annotation>> {
        let idx = self.id_to_idx.get(&id)?;
        self.idx_to_ann.get(*idx as usize)
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn get_idx_by_id(&self, id: OpID) -> Option<AnnIdx> {
        self.id_to_idx.get(&id).copied()
    }
}

/// The annotated text span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    // TODO: use byte slice
    pub insert: String,
    pub attributes: FxHashMap<InternalString, Value>,
}

impl Span {
    pub fn len(&self) -> usize {
        self.insert.len()
    }

    pub fn is_empty(&self) -> bool {
        self.insert.is_empty()
    }

    pub fn as_str(&self) -> &str {
        &self.insert
    }
}

impl Mergeable for Span {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.attributes == rhs.attributes
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.insert.push_str(&rhs.insert);
    }

    fn merge_left(&mut self, _left: &Self) {
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheAnchorSet {
    start: FxHashSet<AnnIdx>,
    end: FxHashSet<AnnIdx>,
}

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct ElemAnchorSet {
    start_before: FxHashSet<AnnIdx>,
    end_before: FxHashSet<AnnIdx>,
    start_after: FxHashSet<AnnIdx>,
    end_after: FxHashSet<AnnIdx>,
}

impl Mergeable for ElemAnchorSet {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.start_after.is_empty()
            && self.end_after.is_empty()
            && rhs.start_before.is_empty()
            && rhs.end_before.is_empty()
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.start_after = rhs.start_after.clone();
        self.end_after = rhs.end_after.clone();
    }

    fn merge_left(&mut self, left: &Self) {
        self.start_before = left.start_before.clone();
        self.end_before = left.end_before.clone();
    }
}

macro_rules! extend_if_not_empty {
    ($a:expr, $b:expr) => {
        if !$b.is_empty() {
            $a.extend($b.iter());
        }
    };
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
        if diff.start.is_empty() && diff.end.is_empty() {
            return;
        }

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
        extend_if_not_empty!(self.start, other.start);
        extend_if_not_empty!(self.end, other.end);
    }

    pub fn union_elem_set(&mut self, other: &ElemAnchorSet) {
        extend_if_not_empty!(self.start, other.start_before);
        extend_if_not_empty!(self.start, other.start_after);
        extend_if_not_empty!(self.end, other.end_before);
        extend_if_not_empty!(self.end, other.end_after);
    }
}

impl ElemAnchorSet {
    pub fn has_start_before(&self) -> bool {
        !self.start_before.is_empty()
    }

    pub fn has_start_after(&self) -> bool {
        !self.start_after.is_empty()
    }

    pub fn contains_start(&self, ann: AnnIdx) -> (bool, bool) {
        let a = self.start_before.contains(&ann);
        let b = self.start_after.contains(&ann);
        (a || b, a)
    }

    /// return (contains_end, is_inclusive)
    pub fn contains_end(&self, ann: AnnIdx) -> (bool, bool) {
        let a = self.end_before.contains(&ann);
        let b = self.end_after.contains(&ann);
        (a || b, b)
    }

    pub fn insert_ann(&mut self, idx: AnnIdx, type_: AnchorType, is_start: bool) {
        if is_start {
            match type_ {
                AnchorType::Before => self.start_before.insert(idx),
                AnchorType::After => self.start_after.insert(idx),
            };
        } else {
            match type_ {
                AnchorType::Before => self.end_before.insert(idx),
                AnchorType::After => self.end_after.insert(idx),
            };
        }
    }

    pub(crate) fn split(&mut self) -> ElemAnchorSet {
        ElemAnchorSet {
            start_before: Default::default(),
            end_before: Default::default(),
            start_after: take(&mut self.start_after),
            end_after: take(&mut self.end_after),
        }
    }

    pub(crate) fn trim(&self, trim_start: bool, trim_end: bool) -> ElemAnchorSet {
        let mut ans = ElemAnchorSet::default();
        if !trim_start {
            ans.start_before = self.start_before.clone();
            ans.end_before = self.end_before.clone();
        }
        if !trim_end {
            ans.start_after = self.start_after.clone();
            ans.end_after = self.end_after.clone();
        }
        ans
    }

    pub(crate) fn trim_(&mut self, trim_start: bool, trim_end: bool) {
        if trim_start {
            self.start_before.clear();
            self.end_before.clear();
        }
        if trim_end {
            self.start_after.clear();
            self.end_after.clear();
        }
    }

    pub fn has_after_anchor(&self) -> bool {
        !self.start_after.is_empty() || !self.end_after.is_empty()
    }

    #[allow(unused)]
    pub fn has_before_anchor(&self) -> bool {
        !self.start_before.is_empty() || !self.end_before.is_empty()
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

    pub fn from_ann(ann: AnnIdx, is_start: bool) -> AnchorSetDiff {
        let mut diff = AnchorSetDiff::default();
        diff.insert(ann, is_start);
        diff
    }
}

impl From<AnchorSetDiff> for CacheDiff {
    fn from(value: AnchorSetDiff) -> Self {
        Self {
            anchor_diff: value,
            len_diff: 0,
            utf16_len_diff: 0,
            line_break_diff: 0,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct StyleCalculator {
    inner: FxHashSet<AnnIdx>,
    cached_start_after: FxHashSet<AnnIdx>,
    cached_end_after: FxHashSet<AnnIdx>,
}

impl StyleCalculator {
    pub fn insert_start(&mut self, start: AnnIdx) {
        self.inner.insert(start);
    }

    pub fn apply_node_start(&mut self, anchor_set: &CacheAnchorSet) {
        if !anchor_set.start.is_empty() {
            for ann in anchor_set.start.iter() {
                self.inner.insert(*ann);
            }
        }
    }

    pub fn apply_node_end(&mut self, anchor_set: &CacheAnchorSet) {
        if !anchor_set.end.is_empty() {
            for ann in anchor_set.end.iter() {
                self.inner.remove(ann);
            }
        }
    }

    pub fn apply_start(&mut self, anchor_set: &ElemAnchorSet) {
        if !anchor_set.start_before.is_empty() {
            for ann in anchor_set.start_before.iter() {
                self.inner.insert(*ann);
            }
        }
        if !anchor_set.end_before.is_empty() {
            for ann in anchor_set.end_before.iter() {
                self.inner.remove(ann);
            }
        }
    }

    pub fn apply_end(&mut self, anchor_set: &ElemAnchorSet) {
        if !anchor_set.start_after.is_empty() {
            for ann in anchor_set.start_after.iter() {
                self.inner.insert(*ann);
            }
        }
        if !anchor_set.end_after.is_empty() {
            for ann in anchor_set.end_after.iter() {
                self.inner.remove(ann);
            }
        }
    }

    pub fn cache_end(&mut self, anchor_set: &ElemAnchorSet) {
        if !anchor_set.start_after.is_empty() {
            for ann in anchor_set.start_after.iter() {
                self.cached_start_after.insert(*ann);
            }
        }
        if !anchor_set.end_after.is_empty() {
            for ann in anchor_set.end_after.iter() {
                self.cached_end_after.insert(*ann);
            }
        }
    }

    pub fn commit_cache(&mut self) {
        if !self.cached_start_after.is_empty() {
            for ann in self.cached_start_after.iter() {
                self.inner.insert(*ann);
            }
            self.cached_start_after.clear();
        }

        if !self.cached_end_after.is_empty() {
            for ann in self.cached_end_after.iter() {
                self.inner.remove(ann);
            }
            self.cached_end_after.clear();
        }
    }

    #[allow(unused)]
    pub fn iter(&self) -> impl Iterator<Item = &AnnIdx> {
        self.inner.iter()
    }

    pub fn calc_styles(&self, manager: &AnnManager) -> impl Iterator<Item = Arc<Annotation>> {
        let mut style_map = FxHashMap::default();
        for ann in self.inner.iter() {
            let ann = manager.get_ann_by_idx(*ann).unwrap();
            let suffix_to_make_inclusive_work = if ann.behavior == Behavior::AllowMultiple {
                Some(ann.id)
            } else {
                None
            };
            match style_map.entry((ann.type_.clone(), suffix_to_make_inclusive_work)) {
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    let (lamport, old_ann) = o.get_mut();
                    if *lamport < ann.range_lamport {
                        *old_ann = ann.clone();
                        *lamport = ann.range_lamport;
                    }
                }
                std::collections::hash_map::Entry::Vacant(t) => {
                    t.insert((ann.range_lamport, ann.clone()));
                }
            }
        }
        style_map.into_iter().map(|(_, (_, ann))| ann)
    }
}

/// This method insert the range anchor to the character at the given index and offset.
pub fn insert_anchor_to_char(
    elements: &mut Vec<Elem>,
    index: usize,
    offset: usize,
    ann: AnnIdx,
    type_: AnchorType,
    is_start: bool,
) {
    match type_ {
        AnchorType::Before => {
            debug_assert!(offset < elements[index].rle_len());
            if offset == 0 {
                elements[index].anchor_set.insert_ann(ann, type_, is_start);
            } else {
                let mut new_elem = elements[index].split(offset);
                new_elem.anchor_set.insert_ann(ann, type_, is_start);
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
}

pub fn insert_anchors_at_same_elem(
    elem: &mut Elem,
    start_offset: usize,
    inclusive_end_offset: usize,
    ann: AnnIdx,
    start_type: AnchorType,
    end_type: AnchorType,
) -> SmallVec<[Elem; 2]> {
    debug_assert!(start_offset <= inclusive_end_offset);
    debug_assert!(inclusive_end_offset < elem.rle_len()); // it's inclusive end, the anchor need be
                                                          // assigned to the character at the end_offset
    let mut ans = SmallVec::new();
    match (start_type, end_type) {
        (AnchorType::Before, AnchorType::Before) => {
            if start_offset == 0 {
                let mut new_elem = elem.split(inclusive_end_offset);
                elem.anchor_set.insert_ann(ann, AnchorType::Before, true);
                new_elem
                    .anchor_set
                    .insert_ann(ann, AnchorType::Before, false);
                ans.push(new_elem);
            } else {
                for v in elem.update_twice(
                    start_offset,
                    inclusive_end_offset,
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
                .update(start_offset, inclusive_end_offset + 1, &mut |elem| {
                    elem.anchor_set.insert_ann(ann, AnchorType::Before, true);
                    elem.anchor_set.insert_ann(ann, AnchorType::After, false);
                })
                .0;
        }
        (AnchorType::After, AnchorType::Before) => {
            debug_assert!(start_offset < inclusive_end_offset);
            let mut middle = elem.split(start_offset + 1); // need to include start_offset at elem
            elem.anchor_set.insert_ann(ann, AnchorType::After, true);
            let len = middle.rle_len();
            let (mut new, _) =
                middle.update(inclusive_end_offset - elem.atom_len(), len, &mut |elem| {
                    elem.anchor_set.insert_ann(ann, end_type, false);
                });
            ans.push(middle);
            ans.append(&mut new);
        }
        (AnchorType::After, AnchorType::After) => {
            debug_assert!(start_offset < inclusive_end_offset);
            let mut middle = elem.split(start_offset + 1);
            elem.anchor_set.insert_ann(ann, AnchorType::After, true);
            let (mut new, _) =
                middle.update(0, inclusive_end_offset + 1 - elem.atom_len(), &mut |elem| {
                    elem.anchor_set.insert_ann(ann, end_type, false);
                });
            ans.push(middle);
            ans.append(&mut new);
        }
    }

    ans
}
