use std::{
    any::Any,
    fmt::Display,
    ops::{Bound, RangeBounds},
    sync::Arc,
};

use append_only_bytes::AppendOnlyBytes;

use generic_btree::{
    rle::{HasLength, Mergeable, Sliceable},
    BTree, MoveEvent, QueryResult,
};
use smallvec::SmallVec;

use crate::{
    rich_text::{op::OpContent, rich_tree::utf16::get_utf16_len},
    Anchor, AnchorType, Annotation, ClientID, Counter, Lamport, OpID, Style,
};

use self::{
    ann::{AnnIdx, AnnManager, Span, StyleCalculator},
    cursor::CursorMap,
    op::{Op, OpStore},
    rich_tree::{query::IndexFinder, rich_tree_btree_impl::RichTreeTrait, CacheDiff, Elem},
};

mod ann;
mod cursor;
mod id_map;
mod iter;
mod op;
mod rich_tree;
#[cfg(test)]
mod test;
#[cfg(feature = "test")]
pub mod test_utils;
mod vv;

pub struct RichText {
    client_id: ClientID,
    bytes: AppendOnlyBytes,
    content: BTree<RichTreeTrait>,
    cursor_map: CursorMap,
    store: OpStore,
    pending_ops: Vec<Op>,
    ann: AnnManager,
    /// this is the styles starting from the very beginning,
    /// which have start anchor of None
    init_styles: StyleCalculator,
}

impl RichText {
    pub fn new(client_id: u64) -> Self {
        let cursor_map: CursorMap = Default::default();
        let update_fn = cursor_map.gen_update_fn();
        let mut content: BTree<RichTreeTrait> = BTree::new();
        content.set_listener(Some(update_fn));
        RichText {
            client_id,
            bytes: AppendOnlyBytes::new(),
            content,
            cursor_map,
            store: OpStore::new(client_id),
            pending_ops: Default::default(),
            ann: AnnManager::new(),
            init_styles: StyleCalculator::default(),
        }
    }

    fn next_id(&self) -> OpID {
        self.store.next_id()
    }

    pub fn insert(&mut self, index: usize, string: &str) {
        fn can_merge_new_slice(
            elem: &Elem,
            id: OpID,
            lamport: u32,
            slice: &append_only_bytes::BytesSlice,
        ) -> bool {
            elem.id.client == id.client
                && elem.id.counter + elem.atom_len() as Counter == id.counter
                && elem.lamport + elem.atom_len() as Lamport == lamport
                && !elem.is_dead()
                && elem.string.can_merge(slice)
        }

        let start = self.bytes.len();
        self.bytes.push_str(string);
        let slice = self.bytes.slice(start..);
        let cache_diff = Some(CacheDiff::new_len_diff(
            string.len() as isize,
            get_utf16_len(&slice) as isize,
        ));
        let id = self.next_id();
        let lamport = self.next_lamport();
        if index == 0 {
            self.store
                .insert_local(OpContent::new_insert(None, slice.clone()));
            self.content.prepend(Elem::new(id, None, lamport, slice));
            return;
        }

        // need to find left op id
        let mut path = self.content.query::<IndexFinder>(&index);
        loop {
            let node = self.content.get_node(path.leaf);
            while path.offset == 0 && path.elem_index > 0 {
                path.elem_index -= 1;
                path.offset = node.elements()[path.elem_index].content_len();
            }

            while path.elem_index > 0 && node.elements()[path.elem_index].is_dead() {
                // avoid left is a tombstone
                path.elem_index -= 1;
                path.offset = node.elements()[path.elem_index].content_len();
            }

            if path.offset == 0 && path.elem_index == 0 {
                while path.offset == 0 && path.elem_index == 0 {
                    // need to go left, because we need to locate the left
                    match self.content.prev_same_level_node(path.leaf) {
                        Some(prev) => {
                            let node = self.content.get_node(prev);
                            path.elem_index = node.len();
                            path.offset = 0;
                            path.leaf = prev;
                        }
                        None => unreachable!(), // we already handled the index==0, this cannot happen
                    }
                }
            } else {
                break;
            }
        }

        let mut left = None;
        let op_slice = slice.clone();
        self.content.update_leaf(path.leaf, |elements| {
            if path.elem_index >= elements.len() {
                // insert at the end
                if let Some(last) = elements.last_mut() {
                    left = Some(last.id_last());
                    if can_merge_new_slice(last, id, lamport, &slice) {
                        // can merge directly
                        last.merge_slice(&slice);
                        self.cursor_map.update(MoveEvent::new_move(path.leaf, last));
                        return (true, cache_diff);
                    }
                    let elem = Elem::new(id, left, lamport, slice);
                    self.cursor_map
                        .update(MoveEvent::new_move(path.leaf, &elem));
                    elements.push(elem);
                    return (true, cache_diff);
                } else {
                    // Elements cannot be empty
                    unreachable!();
                }
            }

            let mut offset = path.offset;
            let mut index = path.elem_index;
            if offset == 0 {
                // ensure not at the beginning of an element
                assert!(index > 0);
                index -= 1;
                offset = elements[index].rle_len();
            }

            if offset == elements[index].rle_len() {
                left = Some(elements[index].id_last());
                if can_merge_new_slice(&elements[index], id, lamport, &slice) {
                    // can merge directly
                    elements[index].merge_slice(&slice);
                    self.cursor_map
                        .update(MoveEvent::new_move(path.leaf, &elements[index]));
                    return (true, cache_diff);
                }

                elements.insert(
                    index + 1,
                    Elem::new(id, Some(elements[index].id_last()), lamport, slice),
                );
                self.cursor_map
                    .update(MoveEvent::new_move(path.leaf, &elements[index + 1]));
                return (true, cache_diff);
            }

            // need to split element
            let right = elements[index].split(offset);
            left = Some(elements[index].id_last());
            elements.splice(
                index + 1..index + 1,
                [
                    Elem::new(id, Some(elements[index].id_last()), lamport, slice),
                    right,
                ],
            );
            self.cursor_map
                .update(MoveEvent::new_move(path.leaf, &elements[index + 1]));
            (true, cache_diff)
        });

        self.store
            .insert_local(OpContent::new_insert(left, op_slice));
    }

    pub fn delete(&mut self, range: impl RangeBounds<usize>) {
        let start = match range.start_bound() {
            Bound::Included(start) => *start,
            Bound::Excluded(start) => *start + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(end) => *end + 1,
            Bound::Excluded(end) => *end,
            Bound::Unbounded => self.len(),
        };
        if start == end {
            return;
        }

        let start_result = self.content.query::<IndexFinder>(&start);
        let end_result = self.content.query::<IndexFinder>(&end);
        let mut deleted = SmallVec::<[(OpID, usize); 4]>::new();

        // deletions don't remove things from the tree, they just mark them as dead
        let mut delete_fn = |elem: &mut Elem| {
            if elem.local_delete() {
                deleted.push((elem.id, elem.rle_len()));
                (-(elem.rle_len() as isize), -(elem.utf16_len as isize))
            } else {
                (0, 0)
            }
        };
        self.content.update_with_filter(
            &start_result..&end_result,
            &mut |slice| {
                match (slice.start, slice.end) {
                    (Some((start_idx, start_offset)), Some((end_idx, end_offset)))
                        if start_idx == end_idx =>
                    {
                        // delete within one element
                        if start_idx >= slice.elements.len() {
                            return (false, None);
                        }

                        let elem = &mut slice.elements[start_idx];
                        if elem.is_dead() {
                            return (false, None);
                        }

                        let (additions, diff) =
                            elem.update(start_offset, end_offset, &mut delete_fn);
                        let (len_diff, utf16_len_diff) = diff.unwrap();
                        if !additions.is_empty() {
                            let len = additions.len();
                            slice
                                .elements
                                .splice(start_idx + 1..start_idx + 1, additions);
                            Elem::try_merge_arr(slice.elements, start_idx, len + 1);
                        } else if start_idx > 0 {
                            Elem::try_merge_arr(slice.elements, start_idx - 1, 2);
                        } else {
                            Elem::try_merge_arr(slice.elements, start_idx, 1);
                        }

                        return (
                            true,
                            Some(CacheDiff::new_len_diff(len_diff, utf16_len_diff)),
                        );
                    }
                    _ => {}
                }

                let mut len_diff = 0;
                let mut utf16_len_diff = 0;
                let mut end = match slice.end {
                    Some((end_idx, end_offset)) => {
                        if end_offset == 0 {
                            end_idx
                        } else {
                            let elem = &mut slice.elements[end_idx];
                            if !elem.is_dead() {
                                let (additions, diff) = elem.update(0, end_offset, &mut delete_fn);
                                if !additions.is_empty() {
                                    slice.elements.splice(end_idx + 1..end_idx + 1, additions);
                                }
                                len_diff += diff.unwrap().0;
                                utf16_len_diff += diff.unwrap().1;
                            }
                            end_idx + 1
                        }
                    }
                    None => slice.elements.len(),
                };

                let start = match slice.start {
                    Some((start_idx, start_offset)) => {
                        if start_offset == 0 {
                            start_idx
                        } else {
                            let elem = &mut slice.elements[start_idx];
                            if !elem.is_dead() && start_offset < elem.rle_len() {
                                let (additions, diff) =
                                    elem.update(start_offset, elem.rle_len(), &mut delete_fn);
                                if !additions.is_empty() {
                                    end += additions.len();
                                    slice
                                        .elements
                                        .splice(start_idx + 1..start_idx + 1, additions);
                                }
                                len_diff += diff.unwrap().0;
                                utf16_len_diff += diff.unwrap().1;
                            }
                            start_idx + 1
                        }
                    }
                    None => 0,
                };

                for elem in slice.elements[start..end].iter_mut() {
                    let diff = delete_fn(elem);
                    len_diff += diff.0;
                    utf16_len_diff += diff.1;
                }

                let begin = start.saturating_sub(2);
                Elem::try_merge_arr(slice.elements, begin, end + 2 - begin);
                (
                    true,
                    Some(CacheDiff::new_len_diff(len_diff, utf16_len_diff)),
                )
            },
            &|cache| cache.len > 0,
        );

        for (start, len) in deleted {
            let op = self
                .store
                .insert_local(OpContent::new_delete(start, len as i32));
            self.cursor_map.register_del(op);
        }
    }

    pub fn annotate(&mut self, range: impl RangeBounds<usize>, style: Style) {
        let start = match range.start_bound() {
            Bound::Included(start) => *start,
            Bound::Excluded(start) => *start + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(end) => *end + 1,
            Bound::Excluded(end) => *end,
            Bound::Unbounded => self.len(),
        };

        if start == end {
            return;
        }

        let start = if style.start_type == AnchorType::Before {
            Some(self.content.query::<IndexFinder>(&start))
        } else {
            if start == 0 {
                None
            } else {
                Some(self.content.query::<IndexFinder>(&start.saturating_sub(1)))
            }
        };
        let end = if style.end_type == AnchorType::Before {
            if end == self.len() {
                None
            } else {
                Some(self.content.query::<IndexFinder>(&(end + 1)))
            }
        } else {
            Some(self.content.query::<IndexFinder>(&end))
        };

        let start_id = start.map(|start| self.get_id_at_pos(start));
        let end_id = end.map(|end| self.get_id_at_pos(end));
        let id = self.next_id();
        let lamport = self.next_lamport();
        let ann = Annotation {
            id,
            range_lamport: (lamport, id),
            range: crate::AnchorRange {
                start: Anchor {
                    id: start_id,
                    type_: style.start_type,
                },
                end: Anchor {
                    id: end_id,
                    type_: style.end_type,
                },
            },
            merge_method: style.merge_method,
            type_: style.type_.clone(),
            meta: None,
        };

        let ann_idx = self.ann.register(Arc::new(ann));
        match (start, end) {
            (None, None) => todo!("start begin cache and end cache"),
            (None, Some(mut end)) => {
                self.content.update_leaf(end.leaf, |elements| {
                    // insert end anchor
                    if end.offset == 0 && end.elem_index > 0 {
                        end.elem_index -= 1;
                        end.offset = elements[end.elem_index].rle_len();
                    }
                    update_elem(elements, end.elem_index, 0, end.offset, &mut |elem| {
                        elem.anchor_set.insert_ann_end(ann_idx, style.end_type);
                    });
                    // Perf, provide ann data
                    (true, None)
                });
                self.init_styles.insert_start(ann_idx);
            }
            (Some(mut start), None) => {
                self.content.update_leaf(start.leaf, |elements| {
                    if start.offset == elements[start.elem_index].rle_len()
                        && start.elem_index + 1 < elements.len()
                    {
                        start.elem_index += 1;
                        start.offset = 0;
                    }
                    let len = elements[start.elem_index].rle_len();
                    update_elem(elements, start.elem_index, start.offset, len, &mut |elem| {
                        elem.anchor_set.insert_ann_start(ann_idx, style.start_type);
                    });
                    // Perf, provide ann data
                    (true, None)
                });
                // the target ends when the doc ends,
                // so we do not need to insert an end anchor
            }
            (Some(start), Some(end)) => {
                self.annotate_given_range(start, end, ann_idx, style);
            }
        }
        // insert new annotation idx to content tree
    }

    fn annotate_given_range(
        &mut self,
        mut start: QueryResult,
        mut end: QueryResult,
        ann_idx: AnnIdx,
        style: Style,
    ) {
        self.content
            .update2_leaf(start.leaf, end.leaf, |elements, from| {
                match from {
                    Some(leaf) => {
                        if leaf == end.leaf {
                            // insert end anchor
                            if end.offset == 0 && end.elem_index > 0 {
                                end.elem_index -= 1;
                                end.offset = elements[end.elem_index].rle_len();
                            }
                            update_elem(elements, end.elem_index, 0, end.offset, &mut |elem| {
                                elem.anchor_set.insert_ann_end(ann_idx, style.end_type);
                            });
                        } else {
                            // insert start anchor
                            debug_assert_eq!(leaf, start.leaf);
                            if start.offset == elements[start.elem_index].rle_len()
                                && start.elem_index + 1 < elements.len()
                            {
                                start.elem_index += 1;
                                start.offset = 0;
                            }
                            let len = elements[start.elem_index].rle_len();
                            update_elem(
                                elements,
                                start.elem_index,
                                start.offset,
                                len,
                                &mut |elem| {
                                    elem.anchor_set.insert_ann_start(ann_idx, style.start_type);
                                },
                            );
                        }

                        true
                    }
                    None => {
                        // start leaf and end leaf is the same
                        if end.offset == 0 && end.elem_index > 0 {
                            end.elem_index -= 1;
                            end.offset = elements[end.elem_index].rle_len();
                        }
                        if start.offset == elements[start.elem_index].rle_len()
                            && start.elem_index + 1 < elements.len()
                        {
                            start.elem_index += 1;
                            start.offset = 0;
                        }

                        if start.elem_index == end.elem_index {
                            let (new_elems, _) = elements[start.elem_index].update(
                                start.offset,
                                end.offset,
                                &mut |elem| {
                                    elem.anchor_set.insert_ann_start(ann_idx, style.start_type);
                                    elem.anchor_set.insert_ann_end(ann_idx, style.end_type);
                                },
                            );
                            if !new_elems.is_empty() {
                                elements
                                    .splice(start.elem_index + 1..start.elem_index + 1, new_elems);
                            }

                            return true;
                        }

                        assert!(end.elem_index > start.elem_index);
                        update_elem(elements, end.elem_index, 0, end.offset, &mut |elem| {
                            elem.anchor_set.insert_ann_end(ann_idx, style.end_type);
                        });
                        let len = elements[start.elem_index].rle_len();
                        update_elem(elements, start.elem_index, start.offset, len, &mut |elem| {
                            elem.anchor_set.insert_ann_start(ann_idx, style.start_type);
                        });

                        true
                    }
                }
            })
    }

    fn get_id_at_pos(&self, pos: QueryResult) -> OpID {
        let node = self.content.get_node(pos.leaf);
        // elem_index may be > elements.len()?
        let elem = &node.elements()[pos.elem_index];
        assert!(pos.offset < elem.rle_len());
        elem.id.inc(pos.offset as u32)
    }

    pub fn iter(&self) -> impl Iterator<Item = Span> + '_ {
        iter::Iter::new(self)
    }

    pub fn iter_range(&self, range: impl RangeBounds<usize>) {
        todo!()
    }

    pub fn len(&self) -> usize {
        self.content.root_cache().len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn utf16_len(&self) -> usize {
        self.content.root_cache().utf16_len
    }

    pub fn apply(&mut self, mut op: Op) {
        let op = match self.store.can_apply(&op) {
            op::CanApply::Yes => op,
            op::CanApply::Trim(len) => {
                op.slice_(len as usize..);
                op
            }
            op::CanApply::Pending => {
                self.pending_ops.push(op);
                return;
            }
            op::CanApply::Seen => return,
        };

        let op_clone = op.clone();
        'apply: {
            match op.content {
                OpContent::Ann(_) => todo!(),
                OpContent::Text(text) => {
                    let scan_start = self.find_next_cursor_of(text.left);
                    if scan_start.is_none() {
                        // insert to the last
                        self.content
                            .push(Elem::new(op.id, text.left, op.lamport, text.text));
                        break 'apply;
                    }
                    let iterator = match scan_start {
                        Some(start) => self.content.iter_range(start..),
                        None => self.content.iter_range(self.content.first_full_path()..),
                    };

                    let mut before = None;
                    // RGA algorithm
                    let ord = (op.lamport, op.id.client);
                    for elem_slice in iterator {
                        let offset = elem_slice.start.unwrap_or(0);
                        let elem_ord = (
                            elem_slice.elem.lamport + offset as Lamport,
                            elem_slice.elem.id.client,
                        );
                        if elem_ord < ord {
                            before = Some(*elem_slice.path());
                            break;
                        }
                    }

                    if let Some(before) = before {
                        self.content.insert_by_query_result(
                            before,
                            Elem::new(op.id, text.left, op.lamport, text.text),
                        );
                    } else {
                        self.content
                            .push(Elem::new(op.id, text.left, op.lamport, text.text));
                    }
                }
                OpContent::Del(del) => {
                    let del = del.positive();
                    self.update_elem_in_id_range(del.start, del.len as usize, |elem| {
                        elem.apply_remote_delete()
                    })
                }
            }
        }

        self.store.insert(op_clone);
    }

    /// Merge data from other data into self
    pub fn merge(&mut self, other: &Self) {
        let vv = self.store.vv();
        let exported = other.store.export(&vv);
        let mut all_ops = Vec::new();
        for (_, mut ops) in exported {
            all_ops.append(&mut ops);
        }
        all_ops.sort_by_key(|x| x.lamport);
        for op in all_ops {
            self.apply(op);
        }
    }

    fn update_elem_in_id_range(
        &mut self,
        mut id: OpID,
        mut len: usize,
        mut f: impl FnMut(&mut Elem),
    ) {
        // dbg!(id, len);
        // dbg!(&self.content);
        // dbg!(&self.cursor_map);
        while len > 0 {
            let (insert_leaf, mut leaf_del_len) = self.cursor_map.get_insert(id).unwrap();
            leaf_del_len = leaf_del_len.min(len);
            let leaf_del_len = leaf_del_len;
            let mut left_len = leaf_del_len;
            // Perf: we may optimize this by only update the cache once
            self.content.update_leaf(insert_leaf, |elements| {
                // dbg!(&elements, leaf_del_len);
                // there may be many pieces need to be updated inside one leaf node
                let mut index = 0;
                loop {
                    let elem = &elements[index];
                    if !elem.overlap(id, leaf_del_len) {
                        index += 1;
                        continue;
                    }

                    let offset = if id.counter > elem.id.counter {
                        (id.counter - elem.id.counter) as usize
                    } else {
                        0
                    };
                    let end = elem
                        .rle_len()
                        .min((id.counter + leaf_del_len as Counter - elem.id.counter) as usize);
                    let (new, _) = elements[index].update(offset, end, &mut f);
                    left_len -= end - offset;
                    if !new.is_empty() {
                        let new_len = new.len();
                        elements.splice(index + 1..index + 1, new);
                        index += new_len;
                    }
                    index += 1;
                    if left_len == 0 {
                        break;
                    }
                }
                assert_eq!(left_len, 0);
                // TODO: Perf can be optimized by merge the cache diff from f
                (true, None)
            });
            id.counter += leaf_del_len as Counter;
            len -= leaf_del_len;
        }
    }

    fn find_next_cursor_of(&self, id: Option<OpID>) -> Option<QueryResult> {
        match id {
            Some(id) => {
                let (mut insert_leaf, _) = self
                    .cursor_map
                    .get_insert(id)
                    .expect("Cannot find target id");
                let mut node = self.content.get_node(insert_leaf);
                let mut elem_index = 0;
                let elements = &node.elements();
                while !elements[elem_index].contains_id(id) {
                    // if range out of bound, then cursor_map is off
                    elem_index += 1;
                }

                // +1 the find the next
                let mut offset = (id.counter - elements[elem_index].id.counter + 1) as usize;
                while offset >= elements[elem_index].atom_len() {
                    offset -= elements[elem_index].atom_len();
                    elem_index += 1;
                    if elem_index >= node.elements().len() {
                        elem_index = 0;
                        let Some(next_leaf) = self.content.next_same_level_node(insert_leaf) else { return None };
                        insert_leaf = next_leaf;
                        node = self.content.get_node(insert_leaf);
                    }
                }

                Some(QueryResult {
                    leaf: insert_leaf,
                    elem_index,
                    offset,
                    found: true,
                })
            }
            None => Some(QueryResult {
                leaf: self.content.first_leaf(),
                elem_index: 0,
                offset: 0,
                found: true,
            }),
        }
    }

    #[inline(always)]
    fn next_lamport(&self) -> u32 {
        self.store.next_lamport()
    }

    pub(crate) fn check(&self) {
        self.content.check();
    }

    pub fn debug_log(&self) {
        println!("Text len = {} (utf16={})", self.len(), self.utf16_len());
        println!("Nodes len = {}", self.content.node_len());
        println!("Op len = {}", self.store.op_len());
        let mut content_inner = format!("{:#?}", &self.content);
        const MAX: usize = 100000;
        if content_inner.len() > MAX {
            for new_len in MAX.. {
                if content_inner.is_char_boundary(new_len) {
                    content_inner.truncate(new_len);
                    break;
                }
            }
        }
        println!("ContentTree = {}", content_inner);
        // println!("Text = {}", self);
        println!("Store = {:#?}", &self.store);
    }

    pub fn check_no_mergeable_neighbor(&self) {
        let mut leaf_idx = Some(self.content.first_leaf());
        while let Some(leaf) = leaf_idx {
            let node = self.content.get_node(leaf);
            let elements = node.elements();
            for i in 0..elements.len() - 1 {
                if elements[i].can_merge(&elements[i + 1]) {
                    self.debug_log();
                    panic!(
                        "Found mergeable neighbor: \n{:#?} \n{:#?}",
                        elements[i],
                        elements[i + 1]
                    );
                }
            }

            leaf_idx = self.content.next_same_level_node(leaf);
        }
    }
}

fn update_elem(
    elements: &mut Vec<Elem>,
    index: usize,
    offset_start: usize,
    offset_end: usize,
    f: &mut impl FnMut(&mut Elem),
) {
    let (new, _) = elements[index].update(offset_start, offset_end, f);
    if !new.is_empty() {
        elements.splice(index + 1..index + 1, new);
    }
}

impl Display for RichText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for span in self.content.iter() {
            if span.is_dead() {
                continue;
            }

            f.write_str(std::str::from_utf8(&span.string).unwrap())?;
        }

        Ok(())
    }
}
