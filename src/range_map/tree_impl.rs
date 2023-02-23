use bitvec::vec::BitVec;
use generic_btree::{
    rle::{self, update_slice, HasLength, Mergeable, Sliceable},
    BTree, BTreeTrait, ElemSlice, FindResult, HeapVec, Query, QueryResult, SmallElemVec, StackVec,
};
use std::{
    collections::BTreeSet,
    ops::{Range, RangeInclusive},
    sync::Arc,
};

use crate::{range_map::AnnPosRelativeToInsert, Annotation, OpID};
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

    fn get_ann_bit_index(&self, id: OpID) -> Option<usize> {
        self.id_to_bit.get(&id).copied()
    }

    fn get_annotation_range(
        &self,
        id: OpID,
    ) -> Option<(RangeInclusive<QueryResult>, Range<usize>)> {
        let index = self.get_ann_bit_index(id)?;
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
            let end_index = self.tree.root_cache().len - end_finder.visited_len;
            Some((start..=end, start_index..end_index))
        }
    }

    fn bit_index_to_ann(&self, ann_bit_index: usize) -> &Arc<Annotation> {
        let annotation = self
            .id_to_ann
            .get(self.bit_to_id.get(ann_bit_index).unwrap())
            .unwrap();
        annotation
    }

    fn insert_empty_span(&mut self, pos: usize, ann_bit_index: BitVec) {
        let elem = Elem {
            ann: ann_bit_index,
            len: 0,
        };

        self.tree.insert::<IndexFinder>(&pos, elem);
    }

    fn annotate_by_range(&mut self, range: Range<QueryResult>, idx: usize) {
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

    // TODO: Perf can use write buffer to speed up
    fn insert_or_delete_ann_inside_range(
        &mut self,
        range: Range<&QueryResult>,
        index: usize,
        is_insert: bool,
    ) {
        self.tree.update(range, &mut |mut slice| {
            update_slice(&mut slice, &mut |x| {
                set_bit(&mut x.ann, index, is_insert);
                true
            })
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Elem {
    ann: BitVec,
    len: usize,
}

impl Elem {
    fn has_bit_index(&self, bit_index: usize) -> bool {
        if bit_index >= self.ann.len() {
            false
        } else {
            self.ann[bit_index]
        }
    }
}

fn or_(ann: &mut BitVec, new_ann: &BitVec) {
    if ann.len() < new_ann.len() {
        ann.resize(new_ann.len(), false);
    }

    *ann |= new_ann;
}

fn and_(ann: &mut BitVec, new_ann: &BitVec) {
    *ann &= new_ann;
}

fn set_bit(v: &mut BitVec, i: usize, b: bool) {
    if i >= v.len() {
        v.resize(i + 1, false);
    }

    v.set(i, b);
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
        let ann = self.ann.clone();
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
        (self.len == 0 && rhs.len == 0) || self.ann.iter_ones().eq(rhs.ann.iter_ones())
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.len += rhs.len;
        if self.len == 0 {
            or_(&mut self.ann, &rhs.ann);
        }
    }

    fn merge_left(&mut self, left: &Self) {
        self.len += left.len;
        if self.len == 0 {
            or_(&mut self.ann, &left.ann);
        }
    }
}

impl RangeMap for Tree {
    fn init() -> Self {
        Self::new()
    }

    fn insert<F>(&mut self, pos: usize, len: usize, mut f: F)
    where
        F: FnMut(&Annotation) -> super::AnnPosRelativeToInsert,
    {
        let mut spans = self
            .tree
            .iter_range(
                self.tree
                    .range::<IndexFinder>(pos.saturating_sub(1)..(pos + 1).min(self.len())),
            )
            .collect::<StackVec<_>>();

        if !spans.is_empty() {
            // pop redundant end if there are any
            loop {
                let last = spans.last().unwrap();
                let len = last.elem.len;
                if last.end == Some(0) && (len != 0 || spans.len() > 3) {
                    spans.pop();
                } else {
                    break;
                }
            }
        }

        dbg!(&spans);
        assert!(spans.len() <= 3);
        if spans.is_empty() {
            drop(spans);
            // TODO: Perf reuse the query
            self.tree.insert::<IndexFinder>(
                &pos,
                Elem {
                    ann: Default::default(),
                    len,
                },
            );
            return;
        } else if spans.len() == 1 {
            let ann = spans[0].elem.ann.clone();
            drop(spans);
            // TODO: Perf reuse the query
            let result = self.tree.query::<IndexFinder>(&pos);
            self.tree.insert_by_query_result(result, Elem { ann, len });
            return;
        }

        // spans.len() == 2 || spans.len() == 3
        debug_assert!(!std::ptr::eq(spans[0].elem, spans[1].elem));
        debug_assert!(spans.iter().filter(|x| x.elem.len == 0).count() <= 1);
        if spans.len() == 3 {
            debug_assert!(!std::ptr::eq(spans[1].elem, spans[2].elem));
            debug_assert!(spans[0].elem.len > 0);
            debug_assert!(spans[2].elem.len > 0);
            debug_assert_eq!(spans[1].elem.len, 0);
        }

        // need to use f to adjust the ranges of annotations
        let left = if spans[0].elem.len > 0 {
            Some(&spans[0])
        } else {
            debug_assert_eq!(spans.len(), 2);
            None
        };
        let middle = if spans.len() == 3 {
            Some(&spans[1])
        } else if spans[0].elem.len == 0 {
            Some(&spans[0])
        } else if spans[1].elem.len == 0 {
            Some(&spans[1])
        } else {
            None
        };
        let right = if spans.len() == 3 {
            Some(&spans[2])
        } else if spans.len() == 2 && spans[1].elem.len > 0 {
            Some(&spans[1])
        } else {
            None
        };

        let mut shared: Option<BitVec> = None;
        for a in left.iter().chain(middle.iter()).chain(right.iter()) {
            match &mut shared {
                Some(shared) => and_(shared, &a.elem.ann),
                None => {
                    shared = Some(a.elem.ann.clone());
                }
            }
        }

        let shared = shared.unwrap();
        let mut new_elem = Elem::default();
        new_elem.len = len;
        let mut next_empty_elem = Elem::default();
        new_elem.ann = shared.clone();
        next_empty_elem.ann = shared.clone();
        let mut middle_annotations = BitVec::new();

        let mut use_next = false;
        // middle
        if let Some(middle) = middle {
            for ann in middle.elem.ann.iter_ones() {
                if shared.get(ann).as_deref().copied().unwrap_or(false) {
                    set_bit(&mut middle_annotations, ann, true);
                    continue;
                }

                match f(self.bit_index_to_ann(ann)) {
                    AnnPosRelativeToInsert::BeforeInsert => {
                        set_bit(&mut middle_annotations, ann, true);
                    }
                    AnnPosRelativeToInsert::AfterInsert => {
                        use_next = true;
                        set_bit(&mut next_empty_elem.ann, ann, true);
                    }
                    AnnPosRelativeToInsert::IncludeInsert => {
                        set_bit(&mut next_empty_elem.ann, ann, true);
                        set_bit(&mut middle_annotations, ann, true);
                        set_bit(&mut new_elem.ann, ann, true);
                    }
                }
            }
        }

        // left
        let use_next = use_next; // make it immutable
        if let Some(left) = left {
            for ann in left.elem.ann.iter_ones() {
                if shared.get(ann).as_deref().copied().unwrap_or(false) {
                    continue;
                }

                match f(self.bit_index_to_ann(ann)) {
                    AnnPosRelativeToInsert::BeforeInsert => {}
                    AnnPosRelativeToInsert::AfterInsert => {
                        // unreachable!()
                    }
                    AnnPosRelativeToInsert::IncludeInsert => {
                        set_bit(&mut middle_annotations, ann, true);
                        set_bit(&mut new_elem.ann, ann, true);
                        if use_next {
                            set_bit(&mut next_empty_elem.ann, ann, true);
                        }
                    }
                }
            }
        }

        // right
        if let Some(right) = right {
            for ann in right.elem.ann.iter_ones() {
                if shared.get(ann).as_deref().copied().unwrap_or(false) {
                    continue;
                }

                match f(self.bit_index_to_ann(ann)) {
                    AnnPosRelativeToInsert::BeforeInsert => {
                        // unreachable!()
                    }
                    AnnPosRelativeToInsert::AfterInsert => {}
                    AnnPosRelativeToInsert::IncludeInsert => {
                        set_bit(&mut middle_annotations, ann, true);
                        set_bit(&mut new_elem.ann, ann, true);
                        if use_next {
                            debug_log::debug_log!("next from right {:?}", &ann);
                            set_bit(&mut next_empty_elem.ann, ann, true);
                        }
                    }
                }
            }
        }

        dbg!(&left, &middle, &right);
        let path = right
            .map(|x| x.path())
            .unwrap_or_else(|| middle.unwrap().path())
            .clone();
        if let Some(middle) = middle {
            let path = middle.path().clone();
            drop(spans);
            self.tree.update(&path..&path, &mut |slice| {
                let index = slice.start.unwrap().0;
                assert_eq!(slice.elements[index].len, 0);
                if slice.elements[index].ann == middle_annotations {
                    false
                } else {
                    slice.elements[index].ann = middle_annotations.clone();
                    true
                }
            });
        } else {
            drop(spans);
        }

        if use_next {
            self.tree
                .insert_many_by_query_result(&path, [new_elem, next_empty_elem]);
        } else {
            self.tree.insert_by_query_result(path, new_elem);
        }
    }

    fn delete(&mut self, pos: usize, len: usize) {
        let mut has_ann = false;
        let mut ann_bit_index: BitVec = BitVec::new();
        // We should leave deleted annotations in the tree, stored inside a empty span.
        // But there may already have empty spans at `pos` and `pos + len`.
        // So the `delete_range` implementation should be able to handle this case.
        for span in self.tree.drain::<IndexFinder>(pos..pos + len) {
            for ann in span.ann.iter_ones() {
                if ann >= ann_bit_index.len() {
                    ann_bit_index.resize(self.bit_to_id.len(), false);
                }
                ann_bit_index.set(ann, true);
                has_ann = true;
            }
        }

        if has_ann {
            let deleted_ann = !self.tree.root_cache().ann.clone();
            or_(&mut ann_bit_index, &deleted_ann);
            if ann_bit_index.any() {
                self.insert_empty_span(pos, ann_bit_index);
            }
        }
    }

    fn annotate(&mut self, pos: usize, len: usize, annotation: Annotation) {
        let range = self.tree.range::<IndexFinder>(pos..pos + len);
        let ann = Arc::new(annotation);
        let idx = self.try_add_ann(ann);
        self.annotate_by_range(range, idx);
    }

    fn adjust_annotation(
        &mut self,
        target_id: OpID,
        lamport: crate::Lamport,
        patch_id: OpID,
        start_shift: Option<(isize, Option<OpID>)>,
        end_shift: Option<(isize, Option<OpID>)>,
    ) {
        if let Some(ann) = self.id_to_ann.get(&target_id) {
            // skip update if the current lamport is larger
            if ann.range_lamport > (lamport, patch_id) {
                return;
            }
            ann
        } else {
            return;
        };
        let mask = self.get_ann_bit_index(target_id).unwrap();
        let mut final_pos = 0;

        // query pos then update
        if let Some((index_shift, _)) = start_shift {
            let Some(( range, index_range )) = self.get_annotation_range(target_id) else { return };
            let (range_start, _) = range.into_inner();
            let new_index = (index_range.start as isize + index_shift) as usize;
            match index_shift.cmp(&0) {
                std::cmp::Ordering::Less => {
                    // expand start
                    let new_start = self.tree.query::<IndexFinder>(&new_index);
                    self.insert_or_delete_ann_inside_range(&new_start..&range_start, mask, true);
                }
                std::cmp::Ordering::Greater => {
                    // shrink start
                    let new_start = self.tree.query::<IndexFinder>(&new_index);
                    final_pos = new_index;
                    self.insert_or_delete_ann_inside_range(&range_start..&new_start, mask, false);
                }
                std::cmp::Ordering::Equal => {}
            }
        }

        if let Some((index_shift, _)) = end_shift {
            let Some(( range, index_range )) = self.get_annotation_range(target_id) else { return };
            let (_, range_end) = range.into_inner();
            let new_index = (index_range.end as isize + index_shift) as usize;
            match index_shift.cmp(&0) {
                std::cmp::Ordering::Less => {
                    // shrink end
                    let new_end = self.tree.query::<IndexFinder>(&new_index);
                    final_pos = new_index;
                    self.insert_or_delete_ann_inside_range(&new_end..&range_end, mask, false);
                }
                std::cmp::Ordering::Greater => {
                    // expand end
                    let new_end = self.tree.query::<IndexFinder>(&new_index);
                    self.insert_or_delete_ann_inside_range(&range_end..&new_end, mask, true);
                }
                std::cmp::Ordering::Equal => {}
            }
        }

        if !*self.tree.root_cache().ann.get(mask).unwrap() {
            // if the annotation is not in the tree, we should insert it
            let mut bits = BitVec::new();
            set_bit(&mut bits, mask, true);
            self.insert_empty_span(final_pos, bits);
        }

        // update annotation range
        // TODO: Perf remove Arc requirement on RangeMap
        let ann = self.id_to_ann.get_mut(&target_id).unwrap();
        let mut new_ann = (**ann).clone();
        new_ann.range_lamport = (lamport, patch_id);
        if let Some((_, start)) = start_shift {
            new_ann.range.start.id = start;
        }
        if let Some((_, end)) = end_shift {
            new_ann.range.end.id = end;
        }

        *ann = Arc::new(new_ann);
    }

    fn delete_annotation(&mut self, id: OpID) {
        // use annotation finder to delete
        let Some((query_range, _)) = self.get_annotation_range(id) else { return };
        let (start, end) = query_range.into_inner();
        let bit_index = self.get_ann_bit_index(id).unwrap();
        self.insert_or_delete_ann_inside_range(&start..&end, bit_index, false);
    }

    fn get_annotations(&self, pos: usize, len: usize) -> Vec<super::Span> {
        let pos = pos.min(self.len());
        let len = len.min(self.len() - pos);
        let mut ans: Vec<Span> = Vec::new();
        let range = self.tree.range::<IndexFinder>(pos..pos + len);
        let mut last_ann = &BitVec::new();
        // FIXME: should use iter_range_with_buffer_unloaded
        for ElemSlice {
            elem, start, end, ..
        } in self.tree.iter_range(range)
        {
            let start = start.unwrap_or(0);
            let end = end.unwrap_or(elem.len);
            if last_ann.iter_ones().eq(elem.ann.iter_ones()) && !ans.is_empty() {
                ans.last_mut().unwrap().len += end - start;
            } else {
                let mut annotations = BTreeSet::new();
                for ann_bit_index in elem.ann.iter_ones() {
                    let annotation = self.bit_index_to_ann(ann_bit_index);
                    annotations.insert(annotation.clone());
                }

                ans.push(Span {
                    annotations,
                    len: end - start,
                })
            }

            last_ann = &elem.ann;
        }

        ans
    }

    fn get_annotation_pos(&self, id: OpID) -> Option<(Arc<Annotation>, std::ops::Range<usize>)> {
        // use annotation finder to delete
        let (_, index_range) = self.get_annotation_range(id)?;
        let ann = self.id_to_ann.get(&id).unwrap();
        Some((ann.clone(), index_range.start..index_range.end))
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

        for elem in elements.iter_mut() {
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

    fn insert(
        elements: &mut HeapVec<Self::Elem>,
        mut index: usize,
        mut offset: usize,
        elem: Self::Elem,
    ) {
        if index < elements.len() && elements[index].len == 0 {
            // prefer inserting after zero-len element.
            // because this is the behavior depended by RangeMap::insert impl
            offset = 0;
            index += 1;
        }

        rle::insert_with_split(elements, index, offset, elem)
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
                        ann: elements[start.elem_index].ann.clone(),
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
        ans
    }
}

struct AnnotationFinderStart {
    target_bit_index: usize,
    visited_len: usize,
}

struct AnnotationFinderEnd {
    target_bit_index: usize,
    visited_len: usize,
}

impl Query<TreeTrait> for AnnotationFinderStart {
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        Self {
            target_bit_index: *target,
            visited_len: 0,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> FindResult {
        for (i, cache) in child_caches.iter().enumerate() {
            if cache.cache.has_bit_index(self.target_bit_index) {
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
            if cache.has_bit_index(self.target_bit_index) {
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
            target_bit_index: *target,
            visited_len: 0,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> FindResult {
        for (i, cache) in child_caches.iter().enumerate().rev() {
            if cache.cache.has_bit_index(self.target_bit_index) {
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
            if cache.has_bit_index(self.target_bit_index) {
                return FindResult::new_found(i, cache.len);
            }
            self.visited_len += cache.len;
        }

        FindResult::new_missing(0, 0)
    }
}

#[cfg(test)]
#[cfg(feature = "test")]
mod tree_impl_tests {
    use std::collections::HashMap;

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
        let mut tree = Tree::new();
        tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
            tree.delete(10, 10);
            assert_eq!(tree.len(), 90);
            tree.delete(0, 90);
            assert_eq!(tree.len(), 0);
            let ans = tree.get_annotations(0, 100);
            assert_eq!(ans, make_spans(vec![(vec![], 0)]));
        }

        #[test]
        fn delete_text_with_annotation_to_empty() {
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
            tree.annotate(5, 10, a(0));
            tree.delete_annotation(id(0));
            let ans = tree.get_annotations(0, 100);
            assert_eq!(ans, make_spans(vec![(vec![], 100)]));
        }

        #[test]
        fn delete_annotation_in_zero_len_span() {
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
            tree.annotate(0, 10, a(0));
            tree.adjust_annotation(id(0), 1, id(1), None, Some((1, Some(id(4)))));
            let span = tree.get_annotations(2, 1)[0].clone();
            let ann = span.annotations.into_iter().next().unwrap();
            assert_eq!(ann.range.end.id, Some(id(4)));
        }

        #[test]
        fn shrink() {
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
        fn shrink_to_create_an_empty_span() {
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
            tree.annotate(0, 10, a(0));
            tree.adjust_annotation(
                id(0),
                1,
                id(2),
                Some((5, Some(id(3)))),
                Some((-5, Some(id(2)))),
            );
            let ans = tree.get_annotations(0, 100);
            assert_span_eq(
                ans,
                make_spans(vec![(vec![], 5), (vec![0], 0), (vec![], 95)]),
            );
        }

        #[test]
        fn expand_from_empty_span_over_empty_span() {
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
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
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
            tree.annotate(10, 10, a(0));
            tree.insert(20, 1, |_| AnnPosRelativeToInsert::AfterInsert);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 10..20);

            tree.insert(19, 1, |_| AnnPosRelativeToInsert::AfterInsert);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 10..21);

            tree.insert(10, 1, |_| AnnPosRelativeToInsert::AfterInsert);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 11..22);
        }

        #[test]
        fn insert_at_edge_with_diff_mark() {
            let mut tree = Tree::new();
            tree.insert(0, 100, |_| AnnPosRelativeToInsert::AfterInsert);
            tree.annotate(10, 10, a(0));

            // not included in annotated range
            tree.insert(20, 1, |_| AnnPosRelativeToInsert::AfterInsert);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 10..20);

            // included in annotated range
            tree.insert(20, 1, |_| AnnPosRelativeToInsert::IncludeInsert);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 10..21);

            // not included in annotated range
            tree.insert(10, 1, |_| AnnPosRelativeToInsert::AfterInsert);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 11..22);

            // included in annotated range
            tree.insert(11, 1, |_| AnnPosRelativeToInsert::IncludeInsert);
            assert_eq!(tree.get_annotation_pos(id(0)).unwrap().1, 11..23);
        }

        #[test]
        fn test_insert_to_zero_len_position() {}

        #[test]
        fn insert_to_tombstones_left() {}

        #[test]
        fn insert_to_tombstones_right() {}

        #[test]
        fn insert_to_tombstones_middle() {}

        #[test]
        fn should_panic_when_tombstones_pos_err() {}

        #[test]
        fn insert_to_beginning_with_empty_span() {}

        #[test]
        fn insert_to_end_with_empty_span() {}

        #[test]
        fn insert_should_create_new_empty_span_if_annotation_need_to_move_right() {}

        #[test]
        fn move_annotations_right_without_creating_empty_span() {}
    }
}