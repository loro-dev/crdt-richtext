use generic_btree::{
    rle::{HasLength, Mergeable, Sliceable},
    BTree, BTreeTrait, ElemSlice, FindResult, HeapVec, Query, QueryResult, SmallElemVec, StackVec,
};
use std::{
    collections::BTreeSet,
    mem::take,
    ops::{Range, RangeInclusive},
    sync::Arc,
};

use crate::{range_map::AnnPosRelativeToInsert, Annotation, Counter, InternalString, OpID};
use fxhash::{FxHashMap, FxHashSet};

use super::{small_set::SmallSetI32, RangeMap, Span};

type AnnIdx = i32;

#[derive(Debug)]
pub struct TreeRangeMap {
    tree: BTree<TreeTrait>,
    id_to_idx: FxHashMap<OpID, AnnIdx>,
    idx_to_ann: Vec<Arc<Annotation>>,
    expected_root_cache: Elem,
}

#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct AnchorSet {
    pub(crate) start: FxHashSet<AnnIdx>,
    /// this is inclusive end. The
    pub(crate) end: FxHashSet<AnnIdx>,
}

impl AnchorSet {
    pub fn union_(&mut self, other: &Self) {
        if other.is_empty() {
            return;
        }

        self.start.extend(other.start.iter());
        self.end.extend(other.end.iter());
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.start.is_empty() && self.end.is_empty()
    }

    pub fn difference(&self, old: &AnchorSet, output: &mut CacheDiff) {
        for ann in self.start.difference(&old.start) {
            output.start.insert(*ann);
        }
        for ann in self.end.difference(&old.end) {
            output.end.insert(*ann);
        }
        for ann in old.start.difference(&self.start) {
            output.start.insert(-*ann);
        }
        for ann in old.end.difference(&self.end) {
            output.end.insert(-*ann);
        }
    }

    pub fn apply_diff(&mut self, diff: &CacheDiff) {
        for ann in diff.start.iter() {
            if ann >= 0 {
                self.start.insert(ann);
            } else {
                self.start.remove(&(-ann));
            }
        }
        for ann in diff.end.iter() {
            if ann >= 0 {
                self.end.insert(ann);
            } else {
                self.end.remove(&(-ann));
            }
        }
    }

    pub const NEW_ELEM_THRESHOLD: i32 = i32::MAX / 2;

    pub fn process_diff(&mut self, child: &AnchorSet) {
        if child.is_empty() {
            return;
        }
        // if the child has an element that is not in the parent, then it is a new element,
        // it will have ann + Self::NEW_ELEM_THRESHOLD

        // if the child has an element that is in the parent,
        // we will mark it as -ann

        // if the parent has an element that is not in the children,
        // then it should be between 1~Self::NEW_ELEM_THRESHOLD
        for &ann in child.start.iter() {
            if self.start.contains(&ann) {
                self.start.insert(-ann);
            } else {
                self.start.insert(ann + Self::NEW_ELEM_THRESHOLD);
            }
        }
        for &ann in child.end.iter() {
            if self.end.contains(&ann) {
                self.end.insert(-ann);
            } else {
                self.end.insert(ann + Self::NEW_ELEM_THRESHOLD);
            }
        }
    }

    pub fn finish_diff_calc(&mut self) -> CacheDiff {
        if self.is_empty() {
            return Default::default();
        }

        // if the child has an element that is not in the parent, then it is a new element,
        // it will have ann + Self::NEW_ELEM_THRESHOLD

        // if the child has an element that is in the parent,
        // we will mark it as -ann

        // if the parent has an element that is not in the children,
        // then it should be between 1~Self::NEW_ELEM_THRESHOLD
        let mut ans = CacheDiff::default();
        for ann in self.start.iter() {
            if *ann > Self::NEW_ELEM_THRESHOLD {
                // this is a new element
                ans.start.insert(*ann - Self::NEW_ELEM_THRESHOLD);
            } else if *ann > 0 && !self.start.contains(&-*ann) {
                // this is a deleted element
                ans.start.insert(-*ann);
            }
        }
        for ann in ans.start.iter() {
            if ann < 0 {
                self.start.remove(&-ann);
            } else {
                self.start.insert(ann);
            }
        }
        for ann in self.end.iter() {
            if *ann > Self::NEW_ELEM_THRESHOLD {
                // this is a new element
                ans.end.insert(*ann - Self::NEW_ELEM_THRESHOLD);
            } else if *ann > 0 && !self.end.contains(&-*ann) {
                // this is a deleted element
                ans.end.insert(-*ann);
            }
        }
        for ann in ans.end.iter() {
            if ann < 0 {
                self.end.remove(&-ann);
            } else {
                self.end.insert(ann);
            }
        }

        self.start
            .retain(|x| *x > 0 && *x < Self::NEW_ELEM_THRESHOLD);
        self.end.retain(|x| *x > 0 && *x < Self::NEW_ELEM_THRESHOLD);
        ans
    }

    pub fn clear(&mut self) {
        self.start.clear();
        self.end.clear();
    }
}

/// If a annotation is inside anchor set, it's either
///
/// - start at the 0 offset position
/// - or end at the len offset position
#[derive(Debug, PartialEq, Eq, Clone, Default)]
struct Elem {
    anchor_set: AnchorSet,
    len: usize,
}
impl Elem {
    fn new(len: usize) -> Elem {
        Elem {
            anchor_set: Default::default(),
            len,
        }
    }

    fn split_right(&mut self, offset: usize) -> Elem {
        let ans = Elem {
            anchor_set: AnchorSet {
                start: Default::default(),
                end: take(&mut self.anchor_set.end),
            },
            len: self.len - offset,
        };
        self.len = offset;
        ans
    }

    fn apply_diff(&mut self, diff: &CacheDiff) {
        self.anchor_set.apply_diff(diff);
        self.len = (self.len as isize + diff.len_diff) as usize;
    }
}

/// The diffing value between two caches.
///
/// It use negative [AnnIdx] to represent subtraction,
/// positive [AnnIdx] to represent addition
#[derive(Default, Debug)]
pub struct CacheDiff {
    pub start: SmallSetI32,
    pub end: SmallSetI32,
    pub len_diff: isize,
}

impl TreeRangeMap {
    fn check(&self) {
        if cfg!(debug_assertions) {
            assert_eq!(&self.expected_root_cache, self.tree.root_cache());
        }
        // self.check_isolated_ann()
    }

    #[allow(unused)]
    pub(crate) fn log_inner(&self) {
        if cfg!(debug_assertions) {
            let mut inner_spans = vec![];
            let mut cache = FxHashSet::default();
            let mut pending_deletion = FxHashSet::default();
            for span in self.tree.iter() {
                for ann in span.anchor_set.start.iter() {
                    assert!(!cache.contains(ann));
                    if !pending_deletion.contains(ann) {
                        cache.insert(*ann);
                    } else {
                        // TODO: Log EMPTY SPAN
                    }
                }
                for ann in span.anchor_set.end.iter() {
                    if !cache.remove(ann) {
                        pending_deletion.insert(*ann);
                    }
                }
                let v: Vec<_> = cache
                    .iter()
                    .map(|x| self.idx_to_ann[*x as usize].clone())
                    .collect();
                inner_spans.push((v, span.len));
            }

            debug_log::debug_dbg!(inner_spans);
        }
    }

    fn insert_elem<F>(&mut self, pos: usize, mut new_elem: Elem, mut f: F)
    where
        F: FnMut(&Annotation) -> super::AnnPosRelativeToInsert,
    {
        let neighbor_range = self
            .tree
            .range::<IndexFinder>(pos.saturating_sub(1)..(pos + 1).min(self.len()));
        let mut spans = self
            .tree
            .iter_range(neighbor_range.clone())
            .collect::<StackVec<_>>();
        if !spans.is_empty() {
            // pop redundant end if there are any
            loop {
                if spans.len() == 1 {
                    break;
                }

                let last = spans.last().unwrap();
                let len = last.elem.len;
                if (last.end == Some(0) && len != 0)
                    || (len == 0 && spans.len() >= 3)
                    || get_slice_len(&spans[0]) == 2
                {
                    spans.pop();
                } else {
                    break;
                }
            }
            loop {
                if spans.len() == 1 {
                    break;
                }

                let first = spans.first().unwrap();
                let len = first.elem.len;
                if (first.start == Some(first.elem.len) && len != 0)
                    || (len == 0 && spans.len() >= 3)
                    || get_slice_len(spans.last().unwrap()) == 2
                {
                    spans.drain(0..1);
                } else {
                    break;
                }
            }
        }
        debug_assert!(
            spans
                .iter()
                .map(|x| { x.end.unwrap_or(x.elem.len) - x.start.unwrap_or(0) })
                .sum::<usize>()
                <= 2
        );
        if spans.is_empty() {
            // empty tree, insert directly
            drop(spans);
            // TODO: Perf reuse the query
            self.tree.insert::<IndexFinder>(&pos, new_elem);
            debug_log::group_end!();
            return;
        } else if spans.len() == 1 {
            // single span, so we know what the annotations of new insertion
            // insert directly
            drop(spans);
            // TODO: Perf reuse the query
            let result = self.tree.query::<IndexFinder>(&pos);
            self.tree.insert_by_query_result(result, new_elem);
            debug_log::group_end!();
            return;
        }
        let mut middles = StackVec::new();
        let mut left = None;
        let mut right = None;
        if spans[0].elem.len == 0 {
            for span in spans.iter() {
                if span.elem.len == 0 {
                    middles.push(span);
                } else {
                    assert!(right.is_none());
                    right = Some(span);
                }
            }
        } else {
            for span in spans.iter() {
                if left.is_none() {
                    left = Some(span);
                } else if span.elem.len == 0 {
                    middles.push(span);
                } else {
                    assert!(right.is_none());
                    right = Some(span);
                }
            }
        }

        let mut next_anchor_set = AnchorSet::default();
        if new_elem.len == 0 && !middles.is_empty() {
            let trim_start = spans[0].elem.len != 0;
            drop(middles);
            drop(spans);
            self.set_middle_empty_spans_annotations(
                neighbor_range,
                new_elem.anchor_set,
                trim_start,
            );
            return;
        }
        let mut middle_annotations = AnchorSet::default();
        let mut use_next = false;
        for middle in middles.iter() {
            for &ann in middle.elem.anchor_set.start.iter() {
                match f(self.idx_to_ann(ann)) {
                    AnnPosRelativeToInsert::Before => {
                        middle_annotations.start.insert(ann);
                    }
                    AnnPosRelativeToInsert::After => {
                        use_next = true;
                        next_anchor_set.start.insert(ann);
                    }
                    AnnPosRelativeToInsert::IncludeInsert => {
                        middle_annotations.start.insert(ann);
                    }
                }
            }
            for &ann in middle.elem.anchor_set.end.iter() {
                match f(self.idx_to_ann(ann)) {
                    AnnPosRelativeToInsert::Before => {
                        middle_annotations.end.insert(ann);
                    }
                    AnnPosRelativeToInsert::After => {
                        new_elem.anchor_set.end.insert(ann);
                    }
                    AnnPosRelativeToInsert::IncludeInsert => {
                        new_elem.anchor_set.end.insert(ann);
                    }
                }
            }
        }

        let use_next = use_next;
        let mut new_end_set = Vec::new();
        if let Some(left) = left {
            for &ann in left.elem.anchor_set.end.iter() {
                match f(self.idx_to_ann(ann)) {
                    AnnPosRelativeToInsert::Before => {}
                    AnnPosRelativeToInsert::After => {
                        new_end_set.push(ann);
                    }
                    AnnPosRelativeToInsert::IncludeInsert => {
                        new_end_set.push(ann);
                    }
                }
            }
        }
        let mut new_start_set = Vec::new();
        if let Some(right) = right {
            for &ann in right.elem.anchor_set.start.iter() {
                match f(self.idx_to_ann(ann)) {
                    AnnPosRelativeToInsert::Before => {
                        new_start_set.push(ann);
                    }
                    AnnPosRelativeToInsert::After => {}
                    AnnPosRelativeToInsert::IncludeInsert => {
                        new_start_set.push(ann);
                    }
                }
            }
        }
        let right_path = right.map(|x| *x.path());
        let left_path = left.map(|x| *x.path());
        let path = right
            .map(|x| *x.path())
            .unwrap_or_else(|| *middles.last().unwrap().path());
        let middle_len = middles.len();
        if middles.last().is_some() {
            let trim_start = spans[0].elem.len != 0;
            drop(middles);
            drop(spans);
            self.set_middle_empty_spans_annotations(neighbor_range, middle_annotations, trim_start);
        } else {
            drop(middles);
            drop(spans);
        }

        for ann in new_start_set {
            let right_path = &right_path.as_ref().unwrap();
            self.tree
                .get_elem_mut(right_path)
                .unwrap()
                .anchor_set
                .start
                .remove(&ann);
            self.tree
                .recursive_update_cache(right_path.leaf, true, None);
            new_elem.anchor_set.start.insert(ann);
        }
        for ann in new_end_set {
            let left_path = &left_path.as_ref().unwrap();
            self.tree
                .get_elem_mut(left_path)
                .unwrap()
                .anchor_set
                .end
                .remove(&ann);
            self.tree.recursive_update_cache(left_path.leaf, true, None);
            new_elem.anchor_set.end.insert(ann);
        }
        if use_next {
            self.tree.insert_many_by_query_result(
                &path,
                [
                    new_elem,
                    Elem {
                        anchor_set: next_anchor_set,
                        len: 0,
                    },
                ],
            );
        } else {
            self.tree.insert_by_query_result(path, new_elem);
        }
        if middle_len > 1 {
            self.purge_redundant_empty_spans(pos)
        }
    }

    fn purge_redundant_empty_spans(&mut self, _start_from: usize) {
        // TODO: purge
        // self.tree
        //     .update(&neighbor_range.start..&neighbor_range.end, &mut |slice| {
        //         let mut start = slice.start.unwrap_or((0, 0));
        //         let mut end = slice.end.unwrap_or((slice.elements.len() - 1, 0));
        //         start.0 = start.0.min(slice.elements.len() - 1);
        //         end.0 = end.0.min(slice.elements.len() - 1);
        //         if slice.elements[start.0..=end.0]
        //             .iter()
        //             .any(|x| x.len == 0 && x.anchor_set.is_empty())
        //         {
        //             slice
        //                 .elements
        //                 .retain(|x| x.len != 0 || !x.anchor_set.is_empty());
        //             true
        //         } else {
        //             false
        //         }
        //     });
    }

    /// Set the annotations of the middle empty spans. This method will only keep one empty span
    ///
    /// - Need to skip the first few non empty spans, (if skip_start_empty_spans=true)
    /// - Annotate all the continuous empty spans after the first non empty spans
    /// - Stop when meet the first non empty span after the continuous empty spans
    fn set_middle_empty_spans_annotations(
        &mut self,
        neighbor_range: Range<QueryResult>,
        middle_anchor_set: AnchorSet,
        skip_start_empty_spans: bool,
    ) {
        let mut meet_non_empty_span = !skip_start_empty_spans;
        let mut visited_zero_span = false;
        let mut done = false;
        self.tree
            .update(&neighbor_range.start..&neighbor_range.end, &mut |slice| {
                if done {
                    return (false, None);
                }

                let start = slice.start.unwrap_or((0, 0));
                let end = slice.end.unwrap_or((slice.elements.len(), 0));
                let mut updated = false;
                for index in start.0..=end.0 {
                    if slice.elements.len() <= index {
                        break;
                    }

                    // skip the first empty spans
                    if slice.elements[index].len == 0 {
                        if !meet_non_empty_span {
                            continue;
                        }
                    } else {
                        meet_non_empty_span = true;
                    }

                    if visited_zero_span && slice.elements[index].len != 0 {
                        // it's the end of the continuous empty spans, terminate here
                        done = true;
                        break;
                    }

                    if slice.elements[index].len == 0 {
                        if visited_zero_span {
                            if !slice.elements[index].anchor_set.is_empty() {
                                updated = true;
                                slice.elements[index].anchor_set.clear();
                            }
                        } else if slice.elements[index].anchor_set != middle_anchor_set {
                            updated = true;
                            slice.elements[index].anchor_set = middle_anchor_set.clone();
                        }
                        visited_zero_span = true;
                    }
                }

                (updated, None)
            });
        assert!(visited_zero_span);
    }

    pub(crate) fn get_all_alive_ann(&self) -> BTreeSet<Arc<Annotation>> {
        self.tree
            .root_cache()
            .anchor_set
            .start
            .iter()
            .map(|x| self.idx_to_ann[*x as usize].clone())
            .collect()
    }

    fn set_anchor_set(&mut self, pos: usize, anchor_set: AnchorSet) {
        if anchor_set.is_empty() {
            return;
        }

        let path = self.tree.query::<IndexFinder>(&pos);
        self.tree.update_leaf(path.leaf, |elements| {
            (set_anchor_set(anchor_set, path, elements), None)
        })
    }
}

fn set_anchor_set(anchor_set: AnchorSet, path: QueryResult, elements: &mut Vec<Elem>) -> bool {
    let mut elem_index = path.elem_index;
    let mut offset = path.offset;
    if !anchor_set.start.is_empty() {
        if elem_index < elements.len() && elements[elem_index].len == offset {
            elem_index += 1;
            offset = 0;
        }
        if elem_index >= elements.len() {
            let mut new_anchor_set = AnchorSet::default();
            for &idx in anchor_set.start.iter() {
                new_anchor_set.start.insert(idx);
            }
            elements.push(Elem {
                anchor_set: new_anchor_set,
                len: 0,
            });
            elem_index = elements.len() - 1;
            offset = 0;
        } else if offset == 0 {
            for &idx in anchor_set.start.iter() {
                elements[elem_index].anchor_set.start.insert(idx);
            }
        } else {
            let mut new_elem = elements[elem_index].split_right(offset);
            for &idx in anchor_set.start.iter() {
                new_elem.anchor_set.start.insert(idx);
            }
            elements.insert(elem_index + 1, new_elem);
            elem_index += 1;
            offset = 0;
        }
    }
    if !anchor_set.end.is_empty() {
        if offset == 0 && elem_index > 0 {
            elem_index -= 1;
            offset = elements[elem_index].len;
        }
        if elem_index == 0 && offset == 0 {
            if !elements.is_empty() && elements[0].len == 0 {
                for &idx in anchor_set.end.iter() {
                    elements[0].anchor_set.end.insert(idx);
                }
            } else {
                let mut new_anchor_set = AnchorSet::default();
                for &idx in anchor_set.end.iter() {
                    new_anchor_set.end.insert(idx);
                }
                elements.insert(
                    0,
                    Elem {
                        anchor_set: new_anchor_set,
                        len: 0,
                    },
                );
            }
        } else if offset == elements[elem_index].len {
            for &idx in anchor_set.end.iter() {
                elements[elem_index].anchor_set.end.insert(idx);
            }
        } else {
            let new_elem = elements[elem_index].split_right(offset);
            for &idx in anchor_set.end.iter() {
                elements[elem_index].anchor_set.end.insert(idx);
            }
            elements.insert(elem_index + 1, new_elem);
        }
    }

    true
}

fn set_end(elements: &mut Vec<Elem>, mut elem_index: usize, mut offset: usize, idx: i32) -> bool {
    if offset == 0 && elem_index > 0 {
        elem_index -= 1;
        offset = elements[elem_index].len;
    }
    if elem_index == 0 && offset == 0 {
        let mut anchor_set = AnchorSet::default();
        anchor_set.end.insert(idx);
        elements.insert(0, Elem { anchor_set, len: 0 });
    } else if offset == elements[elem_index].len {
        elements[elem_index].anchor_set.end.insert(idx);
    } else {
        assert!(offset < elements[elem_index].len);
        let new_elem = elements[elem_index].split_right(offset);
        elements[elem_index].anchor_set.end.insert(idx);
        elements.insert(elem_index + 1, new_elem);
    }

    true
}

fn set_start(elements: &mut Vec<Elem>, mut elem_index: usize, mut offset: usize, idx: i32) -> bool {
    if elem_index < elements.len() && elements[elem_index].len == offset {
        elem_index += 1;
        offset = 0;
    }
    if elem_index >= elements.len() {
        let mut anchor_set = AnchorSet::default();
        anchor_set.start.insert(idx);
        elements.push(Elem { anchor_set, len: 0 });
    } else if offset == 0 {
        elements[elem_index].anchor_set.start.insert(idx);
    } else {
        let mut new_elem = elements[elem_index].split_right(offset);
        new_elem.anchor_set.start.insert(idx);
        elements.insert(elem_index + 1, new_elem);
    }

    true
}

impl TreeRangeMap {
    pub fn new() -> Self {
        let placeholder: Annotation = Annotation {
            id: OpID::new(u64::MAX, Counter::MAX),
            range_lamport: (88, OpID::new(888, 888)),
            range: crate::AnchorRange {
                start: crate::Anchor {
                    id: None,
                    type_: crate::AnchorType::After,
                },
                end: crate::Anchor {
                    id: None,
                    type_: crate::AnchorType::After,
                },
            },
            behavior: crate::Behavior::Delete,
            type_: InternalString::from(""),
            meta: None,
        };
        // Need to make 0 idx unavailable, so insert a placeholder to take the 0 idx.
        let idx_to_ann = vec![Arc::new(placeholder)];

        Self {
            tree: BTree::new(),
            id_to_idx: FxHashMap::default(),
            idx_to_ann,
            expected_root_cache: Default::default(),
        }
    }

    fn try_add_ann(&mut self, ann: Arc<Annotation>) -> AnnIdx {
        let id = ann.id;
        if let Some(idx) = self.id_to_idx.get(&id) {
            *idx
        } else {
            let idx = self.idx_to_ann.len() as AnnIdx;
            self.id_to_idx.insert(id, idx);
            self.idx_to_ann.push(ann);
            self.expected_root_cache.anchor_set.start.insert(idx);
            self.expected_root_cache.anchor_set.end.insert(idx);
            idx
        }
    }

    #[inline(always)]
    fn get_ann_idx(&self, id: OpID) -> Option<AnnIdx> {
        self.id_to_idx.get(&id).copied()
    }

    fn get_annotation_range(
        &self,
        id: OpID,
    ) -> Option<(RangeInclusive<QueryResult>, Range<usize>)> {
        let index = self.get_ann_idx(id)?;
        let (start, start_finder) = self
            .tree
            .query_with_finder_return::<AnnotationFinderStart>(&(index));
        let (end, end_finder) = self
            .tree
            .query_with_finder_return::<AnnotationFinderEnd>(&(index));

        if !start.found {
            None
        } else {
            assert!(end.found);
            let start_index = start_finder.visited_len;
            let end_index = self.tree.root_cache().len - end_finder.visited_len;
            Some((start..=end, start_index..end_index))
        }
    }

    fn idx_to_ann(&self, ann_bit_index: AnnIdx) -> &Arc<Annotation> {
        let annotation = self.idx_to_ann.get(ann_bit_index as usize).unwrap();
        annotation
    }

    fn insert_ann_range(&mut self, range: Range<&QueryResult>, idx: AnnIdx) {
        let start = range.start;
        let end = range.end;
        self.tree
            .update2_leaf(start.leaf, range.end.leaf, |elements, target| {
                let shared_target = target.is_none();
                if shared_target {
                    // start and end are at the same leaf
                    assert!(
                        end.elem_index > start.elem_index
                            || (end.elem_index == start.elem_index && end.offset >= start.offset)
                    );

                    // Assumption: set_end won't affect start path
                    let a = set_end(elements, end.elem_index, end.offset, idx);
                    let b = set_start(elements, start.elem_index, start.offset, idx);
                    a || b
                } else if target.unwrap() == start.leaf {
                    // set start
                    set_start(elements, start.elem_index, start.offset, idx)
                } else {
                    // set end
                    set_end(elements, end.elem_index, end.offset, idx)
                }
            })
    }

    fn id_to_ann(&self, id: OpID) -> Option<&Arc<Annotation>> {
        let index = self.get_ann_idx(id)?;
        self.idx_to_ann.get(index as usize)
    }

    fn id_to_ann_mut(&mut self, id: OpID) -> Option<&mut Arc<Annotation>> {
        let index = self.get_ann_idx(id)?;
        self.idx_to_ann.get_mut(index as usize)
    }

    fn insert_or_delete_ann(&mut self, range: Range<&QueryResult>, index: AnnIdx, is_insert: bool) {
        if is_insert {
            self.insert_ann_range(range, index);
        } else {
            self.tree.update2_leaf(
                range.start.leaf,
                range.end.leaf,
                |elements: &mut Vec<Elem>, target| match target {
                    Some(target) => {
                        if target == range.start.leaf {
                            let e = &mut elements[range.start.elem_index];
                            assert!(e.anchor_set.start.remove(&index));
                            true
                        } else {
                            let e = &mut elements[range.end.elem_index];
                            assert!(e.anchor_set.end.remove(&index));
                            true
                        }
                    }
                    None => {
                        let e = &mut elements[range.start.elem_index];
                        assert!(e.anchor_set.start.remove(&index));
                        let e = &mut elements[range.end.elem_index];
                        assert!(e.anchor_set.end.remove(&index));
                        true
                    }
                },
            );
        }
    }
}

impl Default for TreeRangeMap {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Clone, Default, Debug)]
struct Buffer {
    changes: Vec<isize>,
}

impl HasLength for Elem {
    fn rle_len(&self) -> usize {
        self.len
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
            std::ops::Bound::Unbounded => self.len,
        };
        let len = end - start;
        Self {
            anchor_set: AnchorSet {
                start: if start == 0 {
                    self.anchor_set.start.clone()
                } else {
                    Default::default()
                },
                end: if end == self.len {
                    self.anchor_set.end.clone()
                } else {
                    Default::default()
                },
            },
            len,
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
            std::ops::Bound::Unbounded => self.len,
        };
        let len = end - start;
        if start != 0 {
            self.anchor_set.start.clear();
        }
        if end != self.len {
            self.anchor_set.end.clear();
        }
        self.len = len;
    }
}

impl Mergeable for Elem {
    fn can_merge(&self, rhs: &Self) -> bool {
        (self.len == 0 && rhs.len == 0)
            || (self.anchor_set.end.is_empty() && rhs.anchor_set.start.is_empty())
    }

    fn merge_right(&mut self, rhs: &Self) {
        debug_assert!(self.can_merge(rhs));
        self.len += rhs.len;
        if self.len == 0 {
            self.anchor_set.start.extend(rhs.anchor_set.start.iter());
            self.anchor_set.end.extend(rhs.anchor_set.end.iter());
        } else {
            self.anchor_set.end = rhs.anchor_set.end.clone();
        }
    }

    fn merge_left(&mut self, left: &Self) {
        debug_assert!(left.can_merge(self));
        self.len += left.len;
        if self.len == 0 {
            self.anchor_set.start.extend(left.anchor_set.start.iter());
            self.anchor_set.end.extend(left.anchor_set.end.iter());
        } else {
            self.anchor_set.start = left.anchor_set.start.clone();
        }
    }
}

impl RangeMap for TreeRangeMap {
    #[inline(always)]
    fn init() -> Self {
        Self::new()
    }

    // TODO: refactor: split this method
    fn insert<F>(&mut self, pos: usize, len: usize, f: F)
    where
        F: FnMut(&Annotation) -> super::AnnPosRelativeToInsert,
    {
        debug_log::group!("TreeImpl Insert");
        self.check();
        self.expected_root_cache.len += len;
        let new_elem = Elem::new(len);

        self.insert_elem(pos, new_elem, f);

        self.check();
        debug_log::group_end!();
    }

    fn delete(&mut self, pos: usize, len: usize) {
        self.check();
        self.expected_root_cache.len -= len;
        assert!(pos + len <= self.len());
        let mut anchor_set = AnchorSet::default();

        for span in self.tree.drain::<IndexFinder>(pos..pos + len) {
            anchor_set.union_(&span.anchor_set);
        }

        if !anchor_set.is_empty() {
            self.set_anchor_set(pos, anchor_set);
        }

        self.check();
    }

    fn annotate(&mut self, pos: usize, len: usize, annotation: Annotation) {
        self.check();
        let range = self.tree.range::<IndexFinder>(pos..pos + len);
        let ann = Arc::new(annotation);
        let idx = self.try_add_ann(ann);
        self.insert_ann_range(&range.start..&range.end, idx);
        self.check();
    }

    fn adjust_annotation(
        &mut self,
        target_id: OpID,
        lamport: crate::Lamport,
        patch_id: OpID,
        start_shift: Option<(isize, Option<OpID>)>,
        end_shift: Option<(isize, Option<OpID>)>,
    ) {
        self.check();
        debug_log::group!("AdjustAnnotation {:?}", target_id);
        if let Some(ann) = self.id_to_ann(target_id) {
            // skip update if the current lamport is larger
            if ann.range_lamport > (lamport, patch_id) {
                return;
            }
            ann
        } else {
            return;
        };
        let idx = self.get_ann_idx(target_id).unwrap();
        let Some(( range, index_range )) = self.get_annotation_range(target_id) else { return };
        let (start, end) = range.into_inner();
        self.insert_or_delete_ann(&start..&end, idx, false);

        let new_start = if let Some((index_shift, _)) = start_shift {
            (index_range.start as isize + index_shift) as usize
        } else {
            index_range.start
        };
        let new_end = if let Some((index_shift, _)) = end_shift {
            (index_range.end as isize + index_shift) as usize
        } else {
            index_range.end
        };

        self.log_inner();
        assert!(self.get_annotation_range(target_id).is_none());
        debug_log::debug_log!("Insert new range");
        let new_range = self.tree.range::<IndexFinder>(new_start..new_end);
        self.insert_or_delete_ann(&new_range.start..&new_range.end, idx, true);
        self.log_inner();

        // update annotation's anchors
        // TODO: Perf remove Arc requirement on RangeMap
        let ann = self.id_to_ann_mut(target_id).unwrap();
        let mut new_ann = (**ann).clone();
        new_ann.range_lamport = (lamport, patch_id);
        if let Some((_, start)) = start_shift {
            new_ann.range.start.id = start;
        }
        if let Some((_, end)) = end_shift {
            new_ann.range.end.id = end;
        }

        *ann = Arc::new(new_ann);
        self.check();
        debug_log::group_end!();
    }

    fn delete_annotation(&mut self, id: OpID) {
        self.check();
        self.expected_root_cache
            .anchor_set
            .start
            .remove(self.id_to_idx.get(&id).unwrap());
        self.expected_root_cache
            .anchor_set
            .end
            .remove(self.id_to_idx.get(&id).unwrap());

        let index = self.get_ann_idx(id).unwrap();
        let (range, _) = self.get_annotation_range(id).unwrap();
        self.insert_or_delete_ann(range.start()..range.end(), index, false);
        self.check();
    }

    fn get_annotations(&mut self, mut pos: usize, mut len: usize) -> Vec<super::Span> {
        self.check();
        pos = pos.min(self.len());
        len = len.min(self.len() - pos);
        let (result, finder) = self
            .tree
            .query_with_finder_return::<IndexFinderWithStyles>(&pos);
        let mut styles = finder.started_styles;
        let mut to_delete = FxHashSet::default();
        let old_to_delete = finder.pending_delete;
        for style in old_to_delete {
            if !styles.remove(&style) {
                to_delete.insert(style);
            }
        }
        let end = self.tree.query::<IndexFinder>(&(pos + len));
        let mut ans = Vec::new();

        for elem in self.tree.iter_range(result..end) {
            let mut empty_span_annotations = FxHashSet::default();
            for ann in elem.elem.anchor_set.start.iter() {
                if !to_delete.contains(ann) {
                    styles.insert(*ann);
                } else {
                    empty_span_annotations.insert(*ann);
                }
            }
            if !empty_span_annotations.is_empty() {
                let annotations = empty_span_annotations
                    .union(&styles)
                    .map(|x| self.idx_to_ann[*x as usize].clone())
                    .collect();
                push_to_mergeable_vec_end(
                    &mut ans,
                    Span {
                        annotations,
                        len: 0,
                    },
                );
            }
            let annotations = styles
                .iter()
                .map(|x| self.idx_to_ann[*x as usize].clone())
                .collect();
            let start = elem.start.unwrap_or(0);
            let end = elem.end.unwrap_or(elem.elem.len);
            let len = end - start;
            push_to_mergeable_vec_end(&mut ans, Span { annotations, len });
            for ann in elem.elem.anchor_set.end.iter() {
                if !styles.remove(ann) {
                    to_delete.insert(*ann);
                }
            }
        }
        self.check();
        ans
    }

    fn get_annotation_pos(&self, id: OpID) -> Option<(Arc<Annotation>, std::ops::Range<usize>)> {
        // use annotation finder to delete
        let (_, index_range) = self.get_annotation_range(id)?;
        let ann = self.id_to_ann(id).unwrap();
        Some((ann.clone(), index_range.start..index_range.end))
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.tree.root_cache().len
    }
}

fn get_slice_len(slice: &ElemSlice<Elem>) -> usize {
    let start = slice.start.unwrap_or(0);
    let end = slice.end.unwrap_or(slice.elem.len);
    end - start
}

#[derive(Debug)]
struct TreeTrait;

impl BTreeTrait for TreeTrait {
    type Elem = Elem;
    type WriteBuffer = Buffer;
    type Cache = Elem;

    const MAX_LEN: usize = 8;

    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
        diff: Option<CacheDiff>,
    ) -> Option<CacheDiff> {
        match diff {
            Some(diff) => {
                cache.apply_diff(&diff);
                Some(diff)
            }
            None => {
                cache.len = 0;
                cache.anchor_set.clear();
                for child in caches.iter() {
                    cache.len += child.cache.len;
                    cache.anchor_set.union_(&child.cache.anchor_set);
                }
                None
            }
        }
    }

    fn calc_cache_leaf(
        cache: &mut Self::Cache,
        caches: &[Self::Elem],
        diff: Option<Self::CacheDiff>,
    ) -> CacheDiff {
        let mut len = 0;
        for child in caches.iter() {
            len += child.len;
            cache.anchor_set.process_diff(&child.anchor_set);
        }

        let mut diff = cache.anchor_set.finish_diff_calc();
        diff.len_diff = len as isize - cache.len as isize;
        cache.len = len;
        diff
    }

    fn insert(
        elements: &mut HeapVec<Self::Elem>,
        mut index: usize,
        mut offset: usize,
        mut elem: Self::Elem,
    ) {
        while index < elements.len() && elements[index].len == 0 {
            // always inserting after zero-len element.
            // because this is the behavior depended by RangeMap::insert impl
            offset = 0;
            index += 1;
        }

        let index = index;
        let offset = offset;
        if elements.is_empty() {
            elements.push(elem);
            return;
        }

        if index == elements.len() {
            debug_assert_eq!(offset, 0);
            let last = elements.last_mut().unwrap();
            if last.can_merge(&elem) {
                last.merge_right(&elem);
            } else {
                elements.push(elem);
            }

            return;
        }

        if elements[index].anchor_set.is_empty() && elem.anchor_set.is_empty() {
            elements[index].len += elem.len;
        } else if offset == 0 {
            let target = elements.get_mut(index).unwrap();
            if elem.can_merge(target) {
                target.merge_left(&elem);
            } else {
                elements.insert(index, elem);
            }
        } else if offset == elements[index].rle_len() {
            let target = elements.get_mut(index).unwrap();
            if target.can_merge(&elem) {
                target.merge_right(&elem);
            } else {
                elements.insert(index + 1, elem);
            }
        } else {
            let right = elements[index].slice(offset..);
            elements[index].slice_(..offset);
            let left = elements.get_mut(index).unwrap();
            if left.can_merge(&elem) {
                left.merge_right(&elem);
                if left.can_merge(&right) {
                    left.merge_right(&right);
                } else {
                    elements.insert(index + 1, right);
                }
            } else if elem.can_merge(&right) {
                elem.merge_right(&right);
                elements.insert(index + 1, elem);
            } else {
                elements.splice(index + 1..index + 1, [elem, right]);
            }
        }
    }

    fn insert_batch(
        elements: &mut HeapVec<Self::Elem>,
        mut index: usize,
        mut offset: usize,
        new_elements: impl IntoIterator<Item = Self::Elem>,
    ) where
        Self::Elem: Clone,
    {
        while index < elements.len() && elements[index].len == 0 {
            // always inserting after zero-len element.
            // because this is the behavior depended by RangeMap::insert impl
            offset = 0;
            index += 1;
        }

        if elements.is_empty() {
            elements.splice(0..0, new_elements);
            return;
        }

        // TODO: try merging
        if offset == 0 {
            elements.splice(index..index, new_elements);
        } else if offset == elements[index].rle_len() {
            elements.splice(index + 1..index + 1, new_elements);
        } else {
            let right = elements[index].slice(offset..);
            elements[index].slice_(..offset);
            elements.splice(
                index..index,
                new_elements.into_iter().chain(Some(right).into_iter()),
            );
        }
    }

    type CacheDiff = CacheDiff;

    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        for ann in diff2.start.iter() {
            if diff1.start.contains(-ann) {
                diff1.start.remove(-ann);
            } else {
                diff1.start.insert(ann);
            }
        }
        for ann in diff2.end.iter() {
            if diff1.end.contains(-ann) {
                diff1.end.remove(-ann);
            } else {
                diff1.end.insert(ann);
            }
        }
    }
}

fn push_to_mergeable_vec_end<T: Mergeable>(vec: &mut Vec<T>, elem: T) {
    if let Some(last) = vec.last_mut() {
        if last.can_merge(&elem) {
            last.merge_right(&elem);
            return;
        }
    }
    vec.push(elem);
}

struct IndexFinder {
    left: usize,
}

impl Query<TreeTrait> for IndexFinder {
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        IndexFinder { left: *target }
    }

    /// should prefer zero len element
    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> generic_btree::FindResult {
        if child_caches.is_empty() {
            return FindResult::new_missing(0, self.left);
        }

        let mut last_left = self.left;
        for (i, cache) in child_caches.iter().enumerate() {
            if cache.cache.len == 0 && self.left == 0 {
                return FindResult::new_found(i, self.left);
            }

            if self.left >= cache.cache.len {
                last_left = self.left;
                self.left -= cache.cache.len;
            } else {
                return FindResult::new_found(i, self.left);
            }
        }

        self.left = last_left;
        FindResult::new_missing(child_caches.len() - 1, last_left)
    }

    /// should prefer zero len element
    fn find_element(&mut self, _: &Self::QueryArg, elements: &[Elem]) -> generic_btree::FindResult {
        if elements.is_empty() {
            return FindResult::new_missing(0, self.left);
        }

        let mut last_left = self.left;
        for (i, cache) in elements.iter().enumerate() {
            if cache.len == 0 && self.left == 0 {
                return FindResult::new_found(i, self.left);
            }

            if self.left >= cache.len {
                last_left = self.left;
                self.left -= cache.len;
            } else {
                return FindResult::new_found(i, self.left);
            }
        }

        self.left = last_left;
        FindResult::new_missing(elements.len() - 1, last_left)
    }

    fn delete(
        _: &mut HeapVec<<TreeTrait as BTreeTrait>::Elem>,
        _: &Self::QueryArg,
        _: usize,
        _: usize,
    ) -> Option<<TreeTrait as BTreeTrait>::Elem> {
        unimplemented!("Not supported")
    }

    /// If start or end is zero len element, we should drain it.
    ///
    /// Because IndexFinder scan from left to right and return when left length is zero,
    /// the start is guarantee to include the zero len element.
    ///
    /// The returned annotation set is not accurate on end points, to make the algorithm simpler
    fn drain_range<'a>(
        elements: &'a mut HeapVec<<TreeTrait as BTreeTrait>::Elem>,
        _: &'_ Self::QueryArg,
        _: &'_ Self::QueryArg,
        start: Option<generic_btree::QueryResult>,
        end: Option<generic_btree::QueryResult>,
    ) -> Box<dyn Iterator<Item = Elem> + 'a> {
        fn drain_start(start: QueryResult, elements: &mut [Elem]) -> usize {
            if start.offset == 0 || start.elem_index >= elements.len() {
                start.elem_index
            } else if start.offset == elements[start.elem_index].len {
                start.elem_index + 1
            } else {
                elements[start.elem_index].len = start.offset;
                start.elem_index + 1
            }
        }

        fn drain_end(end: QueryResult, elements: &mut [Elem]) -> usize {
            let end_index = if end.elem_index >= elements.len() {
                end.elem_index
            } else if elements[end.elem_index].len == end.offset {
                end.elem_index + 1
            } else if end.offset == 0 {
                end.elem_index
            } else {
                elements[end.elem_index].len -= end.offset;
                end.elem_index
            };

            // if end is zero len element, we should drain it
            if let Some(end) = elements.get(end_index) {
                if end.len == 0 {
                    end_index + 1
                } else {
                    end_index
                }
            } else {
                end_index
            }
        }

        let mut ans = SmallElemVec::new();
        match (start, end) {
            (None, None) => {
                ans.extend(elements.drain(..));
            }
            (None, Some(end)) => {
                let end = drain_end(end, elements);
                ans.extend(elements.drain(..end));
            }
            (Some(start), None) => {
                let start = drain_start(start, elements);
                ans.extend(elements.drain(start..));
            }
            (Some(start), Some(end)) => {
                if start.elem_index == end.elem_index {
                    let len = end.offset - start.offset;
                    elements[start.elem_index].len -= len;
                    let new_elem = Elem {
                        anchor_set: Default::default(),
                        len,
                    };
                    ans.push(new_elem)
                } else {
                    let start = drain_start(start, elements);
                    let end = drain_end(end, elements);
                    ans.extend(elements.drain(start..end));
                }
            }
        }
        Box::new(ans.into_iter())
    }
}

struct IndexFinderWithStyles {
    left: usize,
    started_styles: FxHashSet<AnnIdx>,
    pending_delete: FxHashSet<AnnIdx>,
}

impl Query<TreeTrait> for IndexFinderWithStyles {
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        IndexFinderWithStyles {
            left: *target,
            started_styles: Default::default(),
            pending_delete: Default::default(),
        }
    }

    /// should prefer zero len element
    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> generic_btree::FindResult {
        if child_caches.is_empty() {
            return FindResult::new_missing(0, self.left);
        }

        let mut last_left = self.left;
        for (i, cache) in child_caches.iter().enumerate() {
            if cache.cache.len == 0 && self.left == 0 {
                return FindResult::new_found(i, self.left);
            }

            if self.left >= cache.cache.len {
                last_left = self.left;
                self.left -= cache.cache.len;
            } else {
                return FindResult::new_found(i, self.left);
            }

            for &ann in cache.cache.anchor_set.start.iter() {
                self.started_styles.insert(ann);
            }
            for ann in cache.cache.anchor_set.end.iter() {
                if !self.started_styles.remove(ann) {
                    self.pending_delete.insert(*ann);
                }
            }
        }

        self.left = last_left;
        FindResult::new_missing(child_caches.len() - 1, last_left)
    }

    /// should prefer zero len element
    fn find_element(&mut self, _: &Self::QueryArg, elements: &[Elem]) -> generic_btree::FindResult {
        if elements.is_empty() {
            return FindResult::new_missing(0, self.left);
        }

        let mut last_left = self.left;
        for (i, cache) in elements.iter().enumerate() {
            if cache.len == 0 && self.left == 0 {
                return FindResult::new_found(i, self.left);
            }

            if self.left >= cache.len {
                last_left = self.left;
                self.left -= cache.len;
            } else {
                return FindResult::new_found(i, self.left);
            }

            for &ann in cache.anchor_set.start.iter() {
                self.started_styles.insert(ann);
            }
            for ann in cache.anchor_set.end.iter() {
                if !self.started_styles.remove(ann) {
                    self.pending_delete.insert(*ann);
                }
            }
        }

        self.left = last_left;
        FindResult::new_missing(elements.len() - 1, last_left)
    }
}

struct AnnotationFinderStart {
    target: AnnIdx,
    visited_len: usize,
}

struct AnnotationFinderEnd {
    target: AnnIdx,
    visited_len: usize,
}

impl Query<TreeTrait> for AnnotationFinderStart {
    type QueryArg = AnnIdx;

    fn init(target: &Self::QueryArg) -> Self {
        Self {
            target: *target,
            visited_len: 0,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> FindResult {
        for (i, cache) in child_caches.iter().enumerate() {
            if cache.cache.anchor_set.start.contains(&self.target) {
                return FindResult::new_found(i, 0);
            }
            self.visited_len += cache.cache.len;
        }

        FindResult::new_missing(0, 0)
    }

    fn find_element(
        &mut self,
        _: &Self::QueryArg,
        elements: &[<TreeTrait as BTreeTrait>::Elem],
    ) -> FindResult {
        for (i, cache) in elements.iter().enumerate() {
            if cache.anchor_set.start.contains(&self.target) {
                return FindResult::new_found(i, 0);
            }
            self.visited_len += cache.len;
        }

        FindResult::new_missing(0, 0)
    }
}

impl Query<TreeTrait> for AnnotationFinderEnd {
    type QueryArg = AnnIdx;

    fn init(target: &Self::QueryArg) -> Self {
        Self {
            target: *target,
            visited_len: 0,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> FindResult {
        for (i, cache) in child_caches.iter().enumerate().rev() {
            if cache.cache.anchor_set.end.contains(&self.target) {
                return FindResult::new_found(i, cache.cache.len);
            }
            self.visited_len += cache.cache.len;
        }

        FindResult::new_missing(0, 0)
    }

    fn find_element(
        &mut self,
        _: &Self::QueryArg,
        elements: &[<TreeTrait as BTreeTrait>::Elem],
    ) -> FindResult {
        for (i, cache) in elements.iter().enumerate().rev() {
            if cache.anchor_set.end.contains(&self.target) {
                return FindResult::new_found(i, cache.len);
            }
            self.visited_len += cache.len;
        }

        FindResult::new_missing(0, 0)
    }
}

impl Mergeable for Span {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.annotations == rhs.annotations || (self.len == 0 && rhs.len == 0)
    }

    fn merge_right(&mut self, rhs: &Self) {
        if self.len == 0 && rhs.len == 0 {
            for v in rhs.annotations.iter() {
                self.annotations.insert(v.clone());
            }
        } else {
            self.len += rhs.len
        }
    }

    fn merge_left(&mut self, left: &Self) {
        if self.len == 0 && left.len == 0 {
            for v in left.annotations.iter() {
                self.annotations.insert(v.clone());
            }
        } else {
            self.len += left.len
        }
    }
}

#[cfg(test)]
#[cfg(feature = "test")]
mod tree_impl_tests {
    use std::collections::{BTreeSet, HashMap};

    use crate::{range_map::AnnPosRelativeToInsert, Anchor, AnchorType};

    use super::*;

    fn as_str(arr: Vec<Span>) -> Vec<String> {
        arr.iter()
            .map(|x| {
                let mut s = x
                    .annotations
                    .iter()
                    .map(|x| x.id.client.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                s.push(';');
                s.push_str(&x.len.to_string());
                s
            })
            .collect()
    }

    fn assert_span_eq(a: Vec<Span>, b: Vec<Span>) {
        assert_eq!(as_str(a), as_str(b));
    }

    fn id(k: u64) -> OpID {
        OpID::new(k, 0)
    }

    fn a(n: u64) -> Annotation {
        Annotation {
            id: id(n),
            range_lamport: (0, id(n)),
            range: crate::AnchorRange {
                start: Anchor {
                    id: Some(id(n)),
                    type_: AnchorType::Before,
                },
                end: Anchor {
                    id: Some(id(n)),
                    type_: AnchorType::Before,
                },
            },
            behavior: crate::Behavior::Merge,
            type_: InternalString::from(""),
            meta: None,
        }
    }

    fn make_spans(spans: Vec<(Vec<u64>, usize)>) -> Vec<Span> {
        let mut map = HashMap::new();
        let mut ans = Vec::new();
        for (annotations, len) in spans.iter() {
            let mut new_annotations = BTreeSet::new();
            for ann in annotations {
                let a = map.entry(*ann).or_insert_with(|| Arc::new(a(*ann))).clone();
                new_annotations.insert(a);
            }
            ans.push(Span {
                annotations: new_annotations,
                len: *len,
            });
        }

        ans
    }

    #[test]
    fn annotate() {
        let mut tree = TreeRangeMap::new();
        tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
        tree.annotate(10, 10, a(2));
        assert_eq!(tree.len(), 100);
        let range = tree.get_annotation_range(id(2));
        assert_eq!(range.unwrap().1, 10..20);
        let ans = tree.get_annotations(0, 100);
        assert_eq!(
            ans,
            make_spans(vec![(vec![], 10), (vec![2], 10), (vec![], 80)])
        );
    }

    mod delete {
        use super::*;

        #[test]
        fn delete_text_to_empty() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.delete(10, 10);
            assert_eq!(tree.len(), 90);
            tree.delete(0, 90);
            assert_eq!(tree.len(), 0);
            let ans = tree.get_annotations(0, 100);
            assert_eq!(ans, make_spans(vec![(vec![], 0)]));
        }

        #[test]
        fn delete_text_with_annotation_to_empty() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(0, 10, a(0));
            tree.annotate(5, 10, a(1));
            tree.annotate(95, 5, a(5));
            tree.delete(0, 100);
            assert_eq!(tree.len(), 0);
            let ans = tree.get_annotations(0, 100);
            assert_eq!(ans, make_spans(vec![(vec![0, 1, 5], 0)]));
        }

        #[test]
        fn delete_text_with_empty_span_at_edge() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(10, 10, a(0));
            tree.delete(10, 10);
            // now we have an empty span
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(
                ans,
                make_spans(vec![(vec![], 10), (vec![0], 0), (vec![], 80)]),
            );

            // should not create another empty span
            tree.delete(10, 10);
            let ans = tree.get_annotations(0, 100);
            assert_eq!(
                ans,
                make_spans(vec![(vec![], 10), (vec![0], 0), (vec![], 70),])
            );

            // should not create another empty span
            tree.delete(0, 10);
            let ans = tree.get_annotations(0, 100);
            assert_eq!(ans, make_spans(vec![(vec![0], 0), (vec![], 70),]));
        }

        #[test]
        fn delete_a_part_of_annotation() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(5, 10, a(0));
            tree.delete(10, 10);
            let ans = tree.get_annotations(0, 100);
            // should not create empty span
            assert_eq!(
                ans,
                make_spans(vec![(vec![], 5), (vec![0], 5), (vec![], 80),])
            );
        }

        #[test]
        fn delete_annotation() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(5, 10, a(0));
            tree.delete_annotation(id(0));
            let ans = tree.get_annotations(0, 100);
            assert_eq!(ans, make_spans(vec![(vec![], 100)]));
        }

        #[test]
        fn delete_annotation_in_zero_len_span() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(0, 10, a(0));
            tree.delete(0, 10);
            // now we have an empty span
            let ans = tree.get_annotations(0, 100);
            assert_eq!(ans, make_spans(vec![(vec![0], 0), (vec![], 90)]));

            // annotation on the empty span is gone
            tree.delete_annotation(id(0));
            let ans = tree.get_annotations(0, 100);
            assert_eq!(ans, make_spans(vec![(vec![], 90)]));
        }

        #[test]
        fn delete_across_several_span() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(0, 10, a(0));
            tree.annotate(5, 10, a(1));
            tree.annotate(6, 10, a(2));
            tree.annotate(7, 10, a(3));
            tree.annotate(8, 10, a(4));
            tree.annotate(9, 10, a(5));
            tree.annotate(10, 10, a(6));
            assert!(tree.get_annotation_range(id(0)).is_some());
            tree.delete_annotation(id(0));
            assert!(tree.get_annotation_range(id(0)).is_none());
            assert!(tree.get_annotation_range(id(1)).is_some());
            tree.delete_annotation(id(1));
            assert!(tree.get_annotation_range(id(1)).is_none());
        }
    }

    mod adjust_annotation_range {
        use super::*;
        #[test]
        fn expand() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(1, 9, a(0));
            // expand end
            tree.adjust_annotation(id(0), 1, id(1), None, Some((1, Some(id(0)))));
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(
                ans,
                make_spans(vec![(vec![], 1), (vec![0], 10), (vec![], 89)]),
            );

            // expand start
            tree.adjust_annotation(id(0), 1, id(1), Some((-1, Some(id(0)))), None);
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(ans, make_spans(vec![(vec![0], 11), (vec![], 89)]));
        }

        #[test]
        fn should_change_anchor_id() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(0, 10, a(0));
            tree.adjust_annotation(id(0), 1, id(1), None, Some((1, Some(id(4)))));
            let span = tree.get_annotations(2, 1)[0].clone();
            let ann = span.annotations.into_iter().next().unwrap();
            assert_eq!(ann.range.end.id, Some(id(4)));
        }

        #[test]
        fn shrink() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(0, 10, a(0));
            // shrink end
            tree.adjust_annotation(id(0), 1, id(1), None, Some((-1, Some(id(0)))));
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(ans, make_spans(vec![(vec![0], 9), (vec![], 91)]));

            // shrink start
            tree.adjust_annotation(id(0), 1, id(1), Some((1, Some(id(0)))), None);
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(
                ans,
                make_spans(vec![(vec![], 1), (vec![0], 8), (vec![], 91)]),
            );
        }

        #[test]
        fn expand_over_empty_span() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(10, 10, a(0));
            tree.delete(10, 10);
            tree.annotate(9, 1, a(1));
            tree.adjust_annotation(id(1), 1, id(2), None, Some((2, Some(id(2)))));
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(
                ans,
                make_spans(vec![
                    (vec![], 9),
                    (vec![1], 1),
                    (vec![0, 1], 0),
                    (vec![1], 2),
                    (vec![], 78),
                ]),
            );
        }

        #[test]
        fn expand_from_empty_span_over_empty_span() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(10, 10, a(0));
            tree.delete(10, 10);
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(
                ans,
                make_spans(vec![(vec![], 10), (vec![0], 0), (vec![], 80)]),
            );
            tree.adjust_annotation(id(0), 1, id(3), None, Some((2, Some(id(3)))));
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(
                ans,
                make_spans(vec![(vec![], 10), (vec![0], 2), (vec![], 78)]),
            );
        }

        #[test]
        fn should_ignore_adjustment_if_lamport_is_too_small() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(10, 10, a(0));
            // set lamport to 2 but not change the range
            tree.adjust_annotation(
                id(0),
                2,
                id(3),
                Some((0, Some(id(1)))),
                Some((0, Some(id(3)))),
            );
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(
                ans,
                make_spans(vec![(vec![], 10), (vec![0], 10), (vec![], 80)]),
            );

            // this operation should have no effect, because lamport 1 < the current lamport 2
            tree.adjust_annotation(
                id(0),
                1,
                id(3),
                Some((-2, Some(id(1)))),
                Some((10, Some(id(3)))),
            );
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(
                ans,
                make_spans(vec![(vec![], 10), (vec![0], 10), (vec![], 80)]),
            );

            // this operation should have effect, because lamport 3 < the current lamport 2
            tree.adjust_annotation(
                id(0),
                3,
                id(3),
                Some((-2, Some(id(1)))),
                Some((10, Some(id(3)))),
            );
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(
                ans,
                make_spans(vec![(vec![], 8), (vec![0], 22), (vec![], 70)]),
            );
        }
    }

    mod insert {
        use super::*;

        #[test]
        fn test_insert_to_annotation() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(10, 10, a(0));
            tree.insert(20, 1, |_| AnnPosRelativeToInsert::Before);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 10..20);

            tree.insert(19, 1, |_| unreachable!());
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 10..21);

            tree.insert(10, 1, |_| AnnPosRelativeToInsert::After);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 11..22);
        }

        #[test]
        fn insert_at_edge_with_diff_mark() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(10, 10, a(0));

            // not included in annotated range
            tree.insert(20, 1, |_| AnnPosRelativeToInsert::Before);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 10..20);

            // included in annotated range
            tree.insert(20, 1, |_| AnnPosRelativeToInsert::IncludeInsert);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 10..21);

            // not included in annotated range
            tree.insert(10, 1, |_| AnnPosRelativeToInsert::After);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 11..22);

            // included in annotated range
            tree.insert(11, 1, |_| AnnPosRelativeToInsert::IncludeInsert);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 11..23);
        }

        #[test]
        fn test_insert_to_zero_len_position() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(10, 10, a(0));
            tree.delete(10, 10);
            tree.insert(10, 1, |_| AnnPosRelativeToInsert::Before);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 10..10);
            tree.insert(10, 1, |_| AnnPosRelativeToInsert::After);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 11..11);
            tree.insert(11, 1, |_| AnnPosRelativeToInsert::IncludeInsert);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 11..12);
        }

        #[test]
        fn test_insert_to_middle_among_tombstones() {
            let mut tree = TreeRangeMap::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
            tree.annotate(0, 100, a(8));
            tree.annotate(10, 1, a(0));
            tree.annotate(11, 1, a(1));
            tree.annotate(12, 1, a(2));
            tree.delete(10, 3);
            tree.insert(10, 1, |ann| {
                if ann.id == id(0) {
                    AnnPosRelativeToInsert::Before
                } else if ann.id == id(2) {
                    AnnPosRelativeToInsert::IncludeInsert
                } else {
                    AnnPosRelativeToInsert::After
                }
            });
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 10..10);
            assert_eq!(tree.get_annotation_pos(id(1)).unwrap().1, 11..11);
            assert_eq!(tree.get_annotation_pos(id(2)).unwrap().1, 10..11);
            assert_eq!(tree.get_annotation_pos(id(8)).unwrap().1, 0..98);
            for ann in tree.get_annotations(0, 98) {
                assert!(ann.annotations.iter().any(|x| x.id == id(8)));
            }
        }

        #[test]
        fn insert_to_beginning_with_empty_span() {
            {
                // after
                let mut tree = TreeRangeMap::new();
                tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
                tree.annotate(0, 1, a(0));
                tree.delete(0, 1);
                tree.insert(0, 1, |_| AnnPosRelativeToInsert::After);
                assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 1..1);
            }
            {
                // include
                let mut tree = TreeRangeMap::new();
                tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
                tree.annotate(0, 1, a(0));
                tree.delete(0, 1);
                tree.insert(0, 1, |_| AnnPosRelativeToInsert::IncludeInsert);
                assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 0..1);
            }
            {
                // before
                let mut tree = TreeRangeMap::new();
                tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
                tree.annotate(0, 1, a(0));
                tree.delete(0, 1);
                tree.insert(0, 1, |_| AnnPosRelativeToInsert::Before);
                assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 0..0);
            }
        }

        #[test]
        fn insert_to_end_with_empty_span() {
            {
                // after
                let mut tree = TreeRangeMap::new();
                tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
                tree.annotate(99, 1, a(0));
                tree.delete(99, 1);
                assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 99..99);
                tree.insert(99, 1, |_| AnnPosRelativeToInsert::After);
                assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 100..100);
            }
            {
                // include
                let mut tree = TreeRangeMap::new();
                tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
                tree.annotate(99, 1, a(0));
                tree.delete(99, 1);
                tree.insert(99, 1, |_| AnnPosRelativeToInsert::IncludeInsert);
                assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 99..100);
            }
            {
                // before
                let mut tree = TreeRangeMap::new();
                tree.insert(0, 100, |_| AnnPosRelativeToInsert::After);
                tree.annotate(99, 1, a(0));
                tree.delete(99, 1);
                tree.insert(99, 1, |_| AnnPosRelativeToInsert::Before);
                assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 99..99);
            }
        }
    }
}
