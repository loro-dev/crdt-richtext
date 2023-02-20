use bitvec::vec::BitVec;
use generic_btree::{
    BTree, BTreeTrait, ElemSlice, FindResult, HeapVec, Query, QueryResult, SmallElemVec,
};
use std::{
    collections::{BTreeSet, HashSet},
    ops::{Index, RangeInclusive},
    sync::Arc,
};

use crate::{Annotation, OpID};
use fxhash::{FxHashMap, FxHashSet};

use super::{RangeMap, Span};

#[derive(Debug)]
pub struct Tree {
    tree: BTree<TreeTrait>,
    id_to_ann: FxHashMap<OpID, Arc<Annotation>>,
    id_to_bit: FxHashMap<OpID, usize>,
    bit_to_id: Vec<OpID>,
}

impl Tree {
    pub fn new() -> Self {
        Self {
            tree: BTree::new(),
            id_to_ann: FxHashMap::default(),
            id_to_bit: FxHashMap::default(),
            bit_to_id: Vec::new(),
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
        let mask = self.get_ann_musk(id)?;
        let (start, start_finder) = self
            .tree
            .query_with_finder_return::<AnnotationFinderStart>(&mask);
        let (end, end_finder) = self
            .tree
            .query_with_finder_return::<AnnotationFinderEnd>(&mask);

        if !start.found {
            None
        } else {
            assert!(end.found);
            let start_index = start_finder.visited_len;
            let end_index = self.tree.root_cache().len - end_finder.visited_len;
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
    fn has_musk(&self, mask: usize) -> bool {
        self.ann[mask]
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
        }
    }

    fn delete(&mut self, pos: usize, len: usize) {
        self.tree.drain::<IndexFinder>(pos..pos + len);
    }

    fn annotate(&mut self, pos: usize, len: usize, annotation: Annotation) {
        let range = self.tree.range::<IndexFinder>(pos..pos + len);
        todo!()
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
        for ElemSlice { elem, start, end } in self
            .tree
            .iter_range(self.tree.range::<IndexFinder>(pos..pos + len))
        {
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

    type Cache = Elem;

    const MAX_LEN: usize = 16;

    fn element_to_cache(element: &Self::Elem) -> Self::Cache {
        element.clone()
    }

    fn calc_cache_internal(caches: &[generic_btree::Child<Self::Cache>]) -> Self::Cache {
        if caches.is_empty() {
            return Default::default();
        }

        let mut len = caches[0].cache.len;
        let mut ann = caches[0].cache.ann.clone();
        for cache in caches.iter().skip(1) {
            ann |= &cache.cache.ann;
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
            ann |= &cache.ann;
            len += cache.len;
        }

        Elem { ann, len }
    }
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
        child_caches: &[generic_btree::Child<Elem>],
    ) -> generic_btree::FindResult {
        for (i, cache) in child_caches.iter().enumerate() {
            if cache.cache.len == 0 && self.left == 0 {
                return FindResult::new_found(i, self.left);
            }

            if self.left >= cache.cache.len {
                self.left -= cache.cache.len;
            } else {
                return FindResult::new_found(i, self.left);
            }
        }

        FindResult::new_missing(child_caches.len(), self.left)
    }

    /// should prefer zero len element
    fn find_element(&mut self, _: &Self::QueryArg, elements: &[Elem]) -> generic_btree::FindResult {
        for (i, cache) in elements.iter().enumerate() {
            if cache.len == 0 && self.left == 0 {
                return FindResult::new_found(i, self.left);
            }

            if self.left >= cache.len {
                self.left -= cache.len;
            } else {
                return FindResult::new_found(i, self.left);
            }
        }

        FindResult::new_missing(elements.len(), self.left)
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
        child_caches: &[generic_btree::Child<<TreeTrait as BTreeTrait>::Cache>],
    ) -> FindResult {
        for (i, cache) in child_caches.iter().enumerate() {
            if cache.cache.has_musk(self.target_musk) {
                FindResult::new_found(i, 0);
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
                FindResult::new_found(i, 0);
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
        child_caches: &[generic_btree::Child<<TreeTrait as BTreeTrait>::Cache>],
    ) -> FindResult {
        for (i, cache) in child_caches.iter().enumerate().rev() {
            if cache.cache.has_musk(self.target_musk) {
                FindResult::new_found(i, 0);
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
                FindResult::new_found(i, 0);
            }
            self.visited_len += cache.len;
        }

        FindResult::new_missing(0, 0)
    }
}

#[cfg(test)]
mod test {
    use bitvec::vec::BitVec;

    #[test]
    fn test_bitvec() {
        let mut a: BitVec<usize> = BitVec::from_slice(&[32usize, 3usize]);
        let b: BitVec<usize> = BitVec::from_slice(&[1]);
        a.resize(501, false);
        a.set(500, true);
    }
}
