use bitvec::vec::BitVec;
use generic_btree::{
    rle::{update_slice, HasLength, Mergeable, Sliceable},
    BTree, BTreeTrait, ElemSlice, FindResult, HeapVec, LengthFinder, Query, QueryResult,
    SmallElemVec,
};
use std::{collections::BTreeSet, ops::RangeInclusive, process::id, sync::Arc};

use crate::{Annotation, OpID};
use fxhash::FxHashMap;

use super::{RangeMap, Span};

pub struct Tree {
    tree: BTree<TreeTrait>,
    id_to_ann: FxHashMap<OpID, Arc<Annotation>>,
    id_to_bit: FxHashMap<OpID, usize>,
    bit_to_id: Vec<OpID>,
}

impl Tree {
    pub fn new() -> Self {
        // make 0 unavailable
        let bit_to_id = vec![OpID {
            client: 13123213213,
            counter: 1233123123,
        }];
        Self {
            tree: BTree::new(),
            id_to_ann: FxHashMap::default(),
            id_to_bit: FxHashMap::default(),
            bit_to_id,
        }
    }

    fn try_add_ann(&mut self, ann: Arc<Annotation>) -> usize {
        let id = ann.id;
        if let Some(bit) = self.id_to_bit.get(&id) {
            *bit
        } else {
            let bit = self.bit_to_id.len();
            self.id_to_bit.insert(id, bit);
            self.bit_to_id.push(id);
            self.id_to_ann.insert(id, ann);
            bit
        }
    }

    fn new_bit_vec(&self) -> BitVec {
        let size = self.bit_to_id.len();
        let mut v = BitVec::with_capacity(size.max(32));
        v.resize(size, false);
        v
    }

    fn get_ann_musk(&self, id: OpID) -> Option<usize> {
        self.id_to_bit.get(&id).copied()
    }

    fn get_annotation_range(
        &self,
        id: OpID,
    ) -> Option<(RangeInclusive<QueryResult>, RangeInclusive<usize>)> {
        let index = self.get_ann_musk(id)?;
        let (start, start_finder) = self
            .tree
            .query_with_finder_return::<AnnotationFinderStart>(&index);
        let (end, end_finder) = self
            .tree
            .query_with_finder_return::<AnnotationFinderEnd>(&index);

        if !start.found {
            None
        } else {
            assert!(end.found);
            let start_index = start_finder.visited_len;
            let end_index = self.tree.root_cache().len - end_finder.visited_len - 1;
            Some((start..=end, start_index..=end_index))
        }
    }

    fn musk_to_ann(&self, ann_musk: usize) -> &Arc<Annotation> {
        let annotation = self
            .id_to_ann
            .get(self.bit_to_id.get(ann_musk).unwrap())
            .unwrap();
        annotation
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Elem {
    ann: BitVec,
    len: usize,
}

impl Elem {
    fn has_musk(&self, musk: usize) -> bool {
        if musk >= self.ann.len() {
            false
        } else {
            self.ann[musk]
        }
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
        let mut ann = self.ann.clone();
        let len = match range.end_bound() {
            std::ops::Bound::Included(x) => *x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.len,
        } - match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => *x + 1,
            std::ops::Bound::Unbounded => 0,
        };
        Self { ann, len }
    }

    fn slice_(&mut self, range: impl std::ops::RangeBounds<usize>)
    where
        Self: Sized,
    {
        let len = match range.end_bound() {
            std::ops::Bound::Included(x) => *x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.len,
        } - match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => *x + 1,
            std::ops::Bound::Unbounded => 0,
        };
        self.len = len;
    }
}

impl Mergeable for Elem {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.ann == rhs.ann
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.len += rhs.len
    }
}

impl RangeMap for Tree {
    fn init() -> Self {
        Self::new()
    }

    fn insert<F>(&mut self, pos: usize, len: usize, f: F)
    where
        F: FnMut(&Annotation) -> super::AnnPosRelativeToInsert,
    {
        let result = self.tree.query::<IndexFinder>(&pos);
        // TODO: handle anchors on the tombstones
        if let Some(elem) = result.elem(&self.tree) {
            self.tree.insert_by_query_result(
                result,
                Elem {
                    ann: elem.ann.clone(),
                    len,
                },
            )
        } else {
            self.tree.insert_by_query_result(
                result,
                Elem {
                    ann: Default::default(),
                    len,
                },
            )
        }
    }

    fn delete(&mut self, pos: usize, len: usize) {
        self.tree.drain::<IndexFinder>(pos..pos + len);
        // TODO: keep tombstones annotations
    }

    fn annotate(&mut self, pos: usize, len: usize, annotation: Annotation) {
        let range = self.tree.range::<IndexFinder>(pos..pos + len);
        let ann = Arc::new(annotation);
        let idx = self.try_add_ann(ann);
        self.tree.update_with_buffer(
            range,
            &mut |mut slice| {
                update_slice(&mut slice, &mut |x| {
                    if idx >= x.ann.len() {
                        x.ann.resize(idx + 10, false);
                    }
                    x.ann.set(idx, true);
                    true
                })
            },
            |buffer, _| {
                if buffer.is_none() {
                    *buffer = Some(Buffer::default());
                }
                buffer.as_mut().unwrap().changes.push(idx as isize);
                true
            },
        );
    }
    fn adjust_annotation(
        &mut self,
        target_id: OpID,
        lamport: crate::Lamport,
        patch_id: OpID,
        start_shift: Option<(isize, Option<OpID>)>,
        end_shift: Option<(isize, Option<OpID>)>,
    ) {
        // query pos then update
        todo!()
    }

    fn delete_annotation(&mut self, id: OpID) {
        // use annotation finder to delete
        todo!()
    }

    fn get_annotations(&self, pos: usize, len: usize) -> Vec<super::Span> {
        let mut ans = Vec::new();
        let range = self.tree.range::<IndexFinder>(pos..pos + len);
        for ElemSlice { elem, start, end } in self.tree.iter_range(range) {
            let start = start.unwrap_or(0);
            let end = end.unwrap_or(elem.len);
            let mut annotations = BTreeSet::new();
            for ann_musk in elem.ann.iter_ones() {
                let annotation = self.musk_to_ann(ann_musk);
                annotations.insert(annotation.clone());
            }

            ans.push(Span {
                annotations,
                len: end - start,
            })
        }

        ans
    }

    fn get_annotation_pos(&self, id: OpID) -> Option<(Arc<Annotation>, std::ops::Range<usize>)> {
        // use annotation finder to delete
        let (_, index_range) = self.get_annotation_range(id)?;
        let ann = self.id_to_ann.get(&id).unwrap();
        Some((ann.clone(), *index_range.start()..*index_range.end() + 1))
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.tree.root_cache().len
    }
}

#[derive(Debug)]
struct TreeTrait;

impl BTreeTrait for TreeTrait {
    type Elem = Elem;
    type WriteBuffer = Buffer;
    type Cache = Elem;

    const MAX_LEN: usize = 4;

    fn element_to_cache(element: &Self::Elem) -> Self::Cache {
        element.clone()
    }

    fn calc_cache_internal(caches: &[generic_btree::Child<Self>]) -> Self::Cache {
        if caches.is_empty() {
            return Default::default();
        }

        let mut len = 0;
        let mut ann: BitVec = Default::default();
        for cache in caches.iter() {
            if let Some(buffer) = &cache.write_buffer {
                let mut new_ann = cache.cache.ann.clone();
                for &change in buffer.changes.iter() {
                    if change > 0 {
                        if change as usize > new_ann.len() {
                            new_ann.resize(change as usize, false);
                        }
                        new_ann.set(change as usize, true);
                    } else {
                        new_ann.set(-change as usize, false);
                    }
                }

                or_(&mut ann, &new_ann);
            } else {
                or_(&mut ann, &cache.cache.ann);
            }

            len += cache.cache.len;
        }

        Elem { ann, len }
    }

    fn calc_cache_leaf(caches: &[Self::Elem]) -> Self::Cache {
        if caches.is_empty() {
            return Default::default();
        }
        let mut len = caches[0].len;
        let mut ann = caches[0].ann.clone();
        for cache in caches.iter().skip(1) {
            or_(&mut ann, &cache.ann);
            len += cache.len;
        }

        Elem { ann, len }
    }

    fn apply_write_buffer_to_elements(
        elements: &mut HeapVec<Self::Elem>,
        write_buffer: &Self::WriteBuffer,
    ) {
        if write_buffer.changes.is_empty() {
            return;
        }

        for (i, elem) in elements.iter_mut().enumerate() {
            for &change in write_buffer.changes.iter() {
                if change > 0 {
                    elem.ann.set(change as usize, true);
                } else {
                    elem.ann.set(-change as usize, false);
                }
            }
        }
    }

    fn apply_write_buffer_to_nodes(
        children: &mut [generic_btree::Child<Self>],
        write_buffer: &Self::WriteBuffer,
    ) {
        if write_buffer.changes.is_empty() {
            return;
        }

        for child in children.iter_mut() {
            let buffer = child.write_buffer.get_or_insert_with(Default::default);
            for &change in write_buffer.changes.iter() {
                buffer.changes.push(change);
            }
        }
    }
}

fn or_(ann: &mut BitVec, new_ann: &BitVec) {
    if ann.len() < new_ann.len() {
        ann.resize(new_ann.len(), false);
    }

    *ann |= new_ann;
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

    fn delete_range(
        elements: &mut HeapVec<<TreeTrait as BTreeTrait>::Elem>,
        _: &Self::QueryArg,
        _: &Self::QueryArg,
        start: Option<generic_btree::QueryResult>,
        end: Option<generic_btree::QueryResult>,
    ) -> SmallElemVec<Elem> {
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
            if end.elem_index >= elements.len() {
                end.elem_index
            } else if elements[end.elem_index].len == end.offset {
                end.elem_index + 1
            } else if end.offset == 0 {
                end.elem_index
            } else {
                elements[end.elem_index].len -= end.offset;
                end.elem_index
            }
        }

        match (start, end) {
            (None, None) => {
                elements.clear();
            }
            (None, Some(end)) => {
                let end = drain_end(end, elements);
                elements.drain(..end);
            }
            (Some(start), None) => {
                let start = drain_start(start, elements);
                elements.drain(start..);
            }
            (Some(start), Some(end)) => {
                if start.elem_index == end.elem_index {
                    elements[start.elem_index].len -= end.offset - start.offset;
                } else {
                    let start = drain_start(start, elements);
                    let end = drain_end(end, elements);
                    elements.drain(start..end);
                }
            }
        }
        SmallElemVec::new()
    }
}

struct AnnotationFinderStart {
    target_musk: usize,
    visited_len: usize,
}

struct AnnotationFinderEnd {
    target_musk: usize,
    visited_len: usize,
}

impl Query<TreeTrait> for AnnotationFinderStart {
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        Self {
            target_musk: *target,
            visited_len: 0,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> FindResult {
        for (i, cache) in child_caches.iter().enumerate() {
            if cache.cache.has_musk(self.target_musk) {
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
            if cache.has_musk(self.target_musk) {
                return FindResult::new_found(i, 0);
            }
            self.visited_len += cache.len;
        }

        FindResult::new_missing(0, 0)
    }
}

impl Query<TreeTrait> for AnnotationFinderEnd {
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        Self {
            target_musk: *target,
            visited_len: 0,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> FindResult {
        for (i, cache) in child_caches.iter().enumerate().rev() {
            if cache.cache.has_musk(self.target_musk) {
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
        for (i, cache) in elements.iter().enumerate().rev() {
            if cache.has_musk(self.target_musk) {
                return FindResult::new_found(i, 0);
            }
            self.visited_len += cache.len;
        }

        FindResult::new_missing(0, 0)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{range_map::AnnPosRelativeToInsert, Anchor, AnchorType};

    use super::*;
    use bitvec::vec::BitVec;

    fn id(k: u64) -> OpID {
        OpID {
            client: k,
            counter: 0,
        }
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
            merge_method: crate::RangeMergeRule::Merge,
            type_: String::new(),
            meta: None,
        }
    }

    fn make_spans(spans: Vec<(Vec<u64>, usize)>) -> Vec<Span> {
        let mut map = HashMap::new();
        let mut ans = Vec::new();
        for i in 0..spans.len() {
            let (annotations, len) = &spans[i];
            let mut new_annotations = BTreeSet::new();
            for ann in annotations {
                let a = map.entry(*ann).or_insert_with(|| Arc::new(a(*ann))).clone();
                let start = i == 0 || spans[i - 1].0.contains(ann);
                let end = i == spans.len() - 1 || spans[i + 1].0.contains(ann);
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
    fn test_annotate() {
        let mut tree = Tree::new();
        tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
        tree.annotate(10, 10, a(2));
        assert_eq!(tree.len(), 100);
        let range = tree.get_annotation_range(id(2));
        assert_eq!(range.unwrap().1, 10..=19);
        let ans = tree.get_annotations(0, 100);
        assert_eq!(
            ans,
            make_spans(vec![(vec![], 10), (vec![2], 10), (vec![], 80)])
        );
    }
}
