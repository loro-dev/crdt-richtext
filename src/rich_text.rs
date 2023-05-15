use std::{
    cmp::Ordering,
    fmt::Display,
    ops::{Bound, RangeBounds},
    sync::Arc,
};

use append_only_bytes::AppendOnlyBytes;

use fxhash::FxHashMap;
use generic_btree::{
    rle::{HasLength, Mergeable, Sliceable},
    BTree, MoveEvent, QueryResult,
};
use smallvec::SmallVec;

use crate::{
    rich_text::{
        ann::insert_anchors_at_same_elem,
        op::OpContent,
        rich_tree::utf16::{get_utf16_len_and_line_breaks, Utf16LenAndLineBreaks},
    },
    Anchor, AnchorType, Annotation, ClientID, Counter, IdSpan, OpID, Style,
};

use self::{
    ann::{insert_anchor_to_char, AnchorSetDiff, AnnIdx, AnnManager, Span, StyleCalculator},
    cursor::CursorMap,
    encoding::{decode, encode},
    op::{Op, OpStore},
    rich_tree::{
        query::{IndexFinder, IndexType, LineStartFinder},
        rich_tree_btree_impl::RichTreeTrait,
        CacheDiff, Elem,
    },
    vv::VersionVector,
};

mod ann;
mod cursor;
mod encoding;
mod id_map;
mod iter;
mod op;
mod rich_tree;
#[cfg(all(test, feature = "test"))]
mod test;
#[cfg(feature = "test")]
pub mod test_utils;
pub mod vv;

pub struct RichText {
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
            bytes: AppendOnlyBytes::new(),
            content,
            cursor_map,
            store: OpStore::new(client_id),
            pending_ops: Default::default(),
            ann: AnnManager::new(),
            init_styles: StyleCalculator::default(),
        }
    }

    #[inline]
    fn next_id(&self) -> OpID {
        self.store.next_id()
    }

    #[inline]
    pub fn insert_utf16(&mut self, index: usize, string: &str) {
        self.insert_inner(index, string, IndexType::Utf16);
    }

    #[inline]
    pub fn insert(&mut self, index: usize, string: &str) {
        self.insert_inner(index, string, IndexType::Utf8);
    }

    fn insert_inner(&mut self, index: usize, string: &str, index_type: IndexType) {
        if string.is_empty() {
            return;
        }

        fn can_merge_new_slice(
            elem: &Elem,
            id: OpID,
            right: Option<OpID>,
            slice: &append_only_bytes::BytesSlice,
        ) -> bool {
            elem.id.client == id.client
                && elem.id.counter + elem.atom_len() as Counter == id.counter
                && elem.right == right
                && !elem.is_dead()
                && elem.string.can_merge(slice)
                && !elem.has_after_anchor()
        }

        let start = self.bytes.len();
        self.bytes.push_str(string);
        let slice = self.bytes.slice(start..);
        let Utf16LenAndLineBreaks { utf16, line_breaks } = get_utf16_len_and_line_breaks(&slice);
        let cache_diff = Some(CacheDiff::new_len_diff(
            string.len() as isize,
            utf16 as isize,
            line_breaks as isize,
        ));
        let id = self.next_id();
        if index == 0 {
            let first_leaf = self.content.first_leaf();
            let right_origin = self
                .content
                .get_node(first_leaf)
                .elements()
                .first()
                .map(|x| x.id);
            self.store
                .insert_local(OpContent::new_insert(None, right_origin, slice.clone()));
            self.content
                .prepend(Elem::new(id, None, right_origin, slice));
            return;
        }

        // need to find left op id
        let path = self.find_ideal_insert_pos(index, index_type);
        let left;
        let right;
        let op_slice = slice.clone();
        {
            // find left and right
            let mut node = self.content.get_node(path.leaf);
            let offset = path.offset;
            let index = path.elem_index;
            if offset != 0 {
                left = Some(node.elements()[index].id.inc((offset - 1) as u32));
            } else {
                left = Some(node.elements()[index - 1].id_last());
            }
            if offset < node.elements()[index].rle_len() {
                right = Some(node.elements()[index].id.inc(offset as u32));
            } else if index + 1 < node.elements().len() {
                right = Some(node.elements()[index + 1].id);
            } else if let Some(next) = self.content.next_same_level_node(path.leaf) {
                node = self.content.get_node(next);
                right = Some(node.elements()[0].id);
            } else {
                right = None;
            }
        }

        self.content.update_leaf(path.leaf, |elements| {
            // insert new element
            debug_assert!(path.elem_index < elements.len());
            let mut offset = path.offset;
            let mut index = path.elem_index;
            if offset == 0 {
                // ensure not at the beginning of an element
                assert!(index > 0);
                index -= 1;
                offset = elements[index].rle_len();
            }

            if offset == elements[index].rle_len() {
                if can_merge_new_slice(&elements[index], id, right, &slice) {
                    // can merge directly
                    elements[index].merge_slice(&slice);
                    self.cursor_map
                        .update(MoveEvent::new_move(path.leaf, &elements[index]));
                    return (true, cache_diff);
                }

                elements.insert(index + 1, Elem::new(id, left, right, slice));
                self.cursor_map
                    .update(MoveEvent::new_move(path.leaf, &elements[index + 1]));
                return (true, cache_diff);
            }

            // need to split element
            let right_half = elements[index].split(offset);
            elements.splice(
                index + 1..index + 1,
                [Elem::new(id, left, right, slice), right_half],
            );
            self.cursor_map
                .update(MoveEvent::new_move(path.leaf, &elements[index + 1]));
            (true, cache_diff)
        });

        self.store
            .insert_local(OpContent::new_insert(left, right, op_slice));
    }

    /// When user insert text at index, there may be tombstones at the given position.
    /// We need to find the ideal position among the tombstones to insert.
    /// Insertion at different position may have different styles.
    ///
    /// The ideal position is:
    ///
    /// - Before tombstones with before anchor
    /// - After tombstones with after anchor
    ///
    /// Sometimes the insertion may not have a ideal position for example an after anchor
    /// may exist before a before anchor.
    ///
    /// The current method is quite straightforward, it will scan from the end backward and stop at
    /// the first position that is not a tombstone or has an after anchor.
    ///
    /// The returned result points to the position the new insertion should be at.
    /// It uses the rle_len() rather than the content_len().
    ///
    /// - rle_len() includes the length of deleted string
    /// - content_len() does not include the length of deleted string
    fn find_ideal_insert_pos(&mut self, index: usize, index_type: IndexType) -> QueryResult {
        let mut path = self.content.query::<IndexFinder>(&(index, index_type));
        loop {
            let node = self.content.get_node(path.leaf);

            // avoid offset == 0, as it makes it hard to find `left` for insertion later
            while path.offset == 0 && path.elem_index > 0 {
                path.elem_index -= 1;
                path.offset = node.elements()[path.elem_index].rle_len();
                if node.elements()[path.elem_index].has_after_anchor() {
                    break;
                }
            }

            // skip tombstones if it does not have after anchor
            while path.elem_index > 0
                && node.elements()[path.elem_index].is_dead()
                && !node.elements()[path.elem_index].has_after_anchor()
            {
                path.elem_index -= 1;
                path.offset = node.elements()[path.elem_index].rle_len();
            }

            if path.offset == 0 && path.elem_index == 0 {
                // cannot find `left` for insertion by this, so we need to go to left node
                while path.offset == 0 && path.elem_index == 0 {
                    match self.content.prev_same_level_node(path.leaf) {
                        Some(prev) => {
                            let node = self.content.get_node(prev);
                            path.elem_index = node.len();
                            path.offset = 0;
                            path.leaf = prev;
                        }
                        None => unreachable!(), // we already handled the index == 0, this cannot happen
                    }
                }
            } else {
                break;
            }
        }
        path
    }

    pub fn delete_utf16(&mut self, range: impl RangeBounds<usize>) {
        self.delete_inner(range, IndexType::Utf16);
    }

    pub fn delete(&mut self, range: impl RangeBounds<usize>) {
        self.delete_inner(range, IndexType::Utf8);
    }

    fn delete_inner(&mut self, range: impl RangeBounds<usize>, index_type: IndexType) {
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

        let start_result = self.content.query::<IndexFinder>(&(start, index_type));
        let end_result = self.content.query::<IndexFinder>(&(end, index_type));
        let mut deleted = SmallVec::<[(OpID, usize); 4]>::new();

        // deletions don't remove things from the tree, they just mark them as dead
        let mut delete_fn = |elem: &mut Elem| {
            if elem.local_delete() {
                deleted.push((elem.id, elem.rle_len()));
                (
                    -(elem.rle_len() as isize),
                    -(elem.utf16_len as isize),
                    -(elem.line_breaks as isize),
                )
            } else {
                (0, 0, 0)
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
                        let (len_diff, utf16_len_diff, line_break_diff) = diff.unwrap();
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
                            Some(CacheDiff::new_len_diff(
                                len_diff,
                                utf16_len_diff,
                                line_break_diff,
                            )),
                        );
                    }
                    _ => {}
                }

                let mut len_diff = 0;
                let mut utf16_len_diff = 0;
                let mut line_break_diff = 0;
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
                                line_break_diff += diff.unwrap().2;
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
                    Some(CacheDiff::new_len_diff(
                        len_diff,
                        utf16_len_diff,
                        line_break_diff,
                    )),
                )
            },
            &|cache| cache.len > 0,
        );

        for (start, len) in deleted {
            self.store
                .insert_local(OpContent::new_delete(start, len as i32));
        }
    }

    /// Annotate the given range with style.
    ///
    /// Under the hood, it will assign anchors to the characters at the given start pos and end pos.
    /// The range start OpID and end OpID are the OpID of those characters;
    ///
    /// Although the arg is a range bound, a `..` range doesn't necessary means the start anchor
    /// and the end anchor is None. Because the range is also depends on the anchor type.
    pub fn annotate_utf16(&mut self, range: impl RangeBounds<usize>, style: Style) {
        self.annotate_inner(range, style, IndexType::Utf16)
    }

    /// Annotate the given range with style.
    ///
    /// Under the hood, it will assign anchors to the characters at the given start pos and end pos.
    /// The range start OpID and end OpID are the OpID of those characters;
    ///
    /// Although the arg is a range bound, a `..` range doesn't necessary means the start anchor
    /// and the end anchor is None. Because the range is also depends on the anchor type.
    pub fn annotate(&mut self, range: impl RangeBounds<usize>, style: Style) {
        self.annotate_inner(range, style, IndexType::Utf8)
    }

    fn annotate_inner(
        &mut self,
        range: impl RangeBounds<usize>,
        style: Style,
        index_type: IndexType,
    ) {
        let start = match range.start_bound() {
            Bound::Included(start) => *start,
            Bound::Excluded(start) => *start + 1,
            Bound::Unbounded => 0,
        };
        let mut inclusive_end = match range.end_bound() {
            Bound::Included(end) => *end,
            Bound::Excluded(end) => *end - 1,
            Bound::Unbounded => self.len_with(index_type) - 1,
        };

        if inclusive_end < start {
            return;
        }

        inclusive_end = inclusive_end.min(self.len_with(index_type) - 1);
        let start = if style.start_type == AnchorType::Before {
            Some(self.content.query::<IndexFinder>(&(start, index_type)))
        } else if start == 0 {
            None
        } else {
            Some(
                self.content
                    .query::<IndexFinder>(&(start.saturating_sub(1), index_type)),
            )
        };
        let inclusive_end = if style.end_type == AnchorType::Before {
            if inclusive_end + 1 >= self.len_with(index_type) {
                None
            } else {
                Some(
                    self.content
                        .query::<IndexFinder>(&(inclusive_end + 1, index_type)),
                )
            }
        } else {
            Some(
                self.content
                    .query::<IndexFinder>(&(inclusive_end, index_type)),
            )
        };

        let start_id = start.map(|start| self.get_id_at_pos(start));
        let end_id = inclusive_end.map(|end| self.get_id_at_pos(end));
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
            behavior: style.behavior,
            type_: style.type_.clone(),
            meta: None,
        };

        let ann = Arc::new(ann);
        let ann_idx = self.ann.register(ann.clone());

        // insert new annotation idx to content tree
        match (start, inclusive_end) {
            (Some(start), Some(end)) => {
                self.annotate_given_range(start, end, ann_idx, style);
            }
            (Some(start), None) => {
                self.content.update_leaf(start.leaf, |elements| {
                    ann::insert_anchor_to_char(
                        elements,
                        start.elem_index,
                        start.offset,
                        ann_idx,
                        style.start_type,
                        true,
                    );
                    (true, Some(AnchorSetDiff::from_ann(ann_idx, true).into()))
                });
                // the target ends when the doc ends,
                // so we do not need to insert an end anchor
            }
            (None, Some(end)) => {
                self.content.update_leaf(end.leaf, |elements| {
                    ann::insert_anchor_to_char(
                        elements,
                        end.elem_index,
                        end.offset,
                        ann_idx,
                        style.end_type,
                        false,
                    );
                    (true, Some(AnchorSetDiff::from_ann(ann_idx, false).into()))
                });
                self.init_styles.insert_start(ann_idx);
            }
            (None, None) => {
                self.init_styles.insert_start(ann_idx);
                // the target ends when the doc ends, so we do not need to insert an end anchor
            }
        }

        // register op to store
        self.store.insert_local(OpContent::new_ann(ann));
    }

    fn annotate_given_range(
        &mut self,
        start: QueryResult,
        end: QueryResult,
        ann_idx: AnnIdx,
        style: Style,
    ) {
        self.content
            .update2_leaf(start.leaf, end.leaf, |elements, from| {
                match from {
                    Some(leaf) => {
                        if leaf == end.leaf {
                            // insert end anchor
                            ann::insert_anchor_to_char(
                                elements,
                                end.elem_index,
                                end.offset,
                                ann_idx,
                                style.end_type,
                                false,
                            );
                        } else {
                            // insert start anchor
                            debug_assert_eq!(leaf, start.leaf);
                            ann::insert_anchor_to_char(
                                elements,
                                start.elem_index,
                                start.offset,
                                ann_idx,
                                style.start_type,
                                true,
                            );
                        }

                        true
                    }
                    None => {
                        if start.elem_index == end.elem_index {
                            assert_ne!(end.offset, elements[start.elem_index].rle_len());
                            let new = insert_anchors_at_same_elem(
                                &mut elements[start.elem_index],
                                start.offset,
                                end.offset,
                                ann_idx,
                                style.start_type,
                                style.end_type,
                            );

                            elements.splice(start.elem_index + 1..start.elem_index + 1, new);
                            return true;
                        }

                        assert!(end.elem_index > start.elem_index);
                        ann::insert_anchor_to_char(
                            elements,
                            end.elem_index,
                            end.offset,
                            ann_idx,
                            style.end_type,
                            false,
                        );
                        ann::insert_anchor_to_char(
                            elements,
                            start.elem_index,
                            start.offset,
                            ann_idx,
                            style.start_type,
                            true,
                        );

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

    pub fn get_spans(&self) -> Vec<Span> {
        self.iter().collect()
    }

    pub fn iter_range(&self, _range: impl RangeBounds<usize>) {
        todo!()
    }

    pub fn len(&self) -> usize {
        self.content.root_cache().len as usize
    }

    pub fn len_utf16(&self) -> usize {
        self.content.root_cache().utf16_len as usize
    }

    fn len_with(&self, index_type: IndexType) -> usize {
        match index_type {
            IndexType::Utf8 => self.content.root_cache().len as usize,
            IndexType::Utf16 => self.content.root_cache().utf16_len as usize,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn utf16_len(&self) -> usize {
        self.content.root_cache().utf16_len as usize
    }

    pub fn export(&self, vv: &VersionVector) -> Vec<u8> {
        encode(self.store.export(vv))
    }

    pub fn import(&mut self, data: &[u8]) {
        self.import_inner(decode(data));
    }

    fn apply(&mut self, op: Op) {
        debug_log::group!("apply op");
        'apply: {
            match &op.content {
                OpContent::Ann(ann) => {
                    let ann_idx = self.ann.register(ann.clone());
                    match ann.range.start.id {
                        Some(start_id) => {
                            let cursor = self.find_cursor(start_id);
                            self.content.update_leaf(cursor.leaf, |elements| {
                                let index = cursor.elem_index;
                                let offset = cursor.offset;
                                let type_ = ann.range.start.type_;
                                let is_start = true;
                                insert_anchor_to_char(
                                    elements, index, offset, ann_idx, type_, is_start,
                                );
                                (
                                    true,
                                    Some(AnchorSetDiff::from_ann(ann_idx, is_start).into()),
                                )
                            });
                        }
                        None => {
                            self.init_styles.insert_start(ann_idx);
                        }
                    }

                    if let Some(end_id) = ann.range.end.id {
                        let cursor = self.find_cursor(end_id);
                        self.content.update_leaf(cursor.leaf, |elements| {
                            let index = cursor.elem_index;
                            let offset = cursor.offset;
                            let type_ = ann.range.end.type_;
                            let is_start = false;
                            insert_anchor_to_char(
                                elements, index, offset, ann_idx, type_, is_start,
                            );
                            (
                                true,
                                Some(AnchorSetDiff::from_ann(ann_idx, is_start).into()),
                            )
                        });
                    }
                }
                OpContent::Text(text) => {
                    let right = match self.find_right(text, &op) {
                        Some(value) => value,
                        None => break 'apply,
                    };

                    if let Some(right) = right {
                        self.content.insert_by_query_result(
                            right,
                            Elem::new(op.id, text.left, text.right, text.text.clone()),
                        );
                    } else {
                        self.content.push(Elem::new(
                            op.id,
                            text.left,
                            text.right,
                            text.text.clone(),
                        ));
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

        debug_log::group_end!();
    }

    fn find_right(&mut self, elt: &op::TextInsertOp, op: &Op) -> Option<Option<QueryResult>> {
        // We use Fugue algorithm here, it has the property of "maximal non-interleaving"
        // See paper *The Art of the Fugue: Minimizing Interleaving in Collaborative Text Editing*
        let scan_start = self.find_next_cursor_of(elt.left);
        if scan_start.is_none() {
            // insert to the last
            self.content
                .push(Elem::new(op.id, elt.left, elt.right, elt.text.clone()));
            return None;
        }
        let scan_start = scan_start.unwrap();
        let iterator = self.content.iter_range(scan_start..);
        let elt_left_origin = elt.left;
        let elt_right_origin = elt.right;
        let mut elt_right_parent: Option<Option<QueryResult>> = None; // calc lazily
        let mut visited_id_spans: SmallVec<[IdSpan; 8]> = SmallVec::new();
        let mut left = None;
        let mut scanning = false;
        for o_slice in iterator {
            // a slice may contains several ops
            let offset = o_slice.start.unwrap_or(0);
            let o_left_origin = if offset == 0 {
                o_slice.elem.left
            } else {
                Some(o_slice.elem.id.inc(offset as u32 - 1))
            };

            let end_offset = if let Some(right) = elt.right {
                if o_slice.elem.contains_id(right) {
                    (right.counter - o_slice.elem.id.counter) as usize
                } else {
                    o_slice.elem.rle_len()
                }
            } else {
                o_slice.elem.rle_len()
            };

            if end_offset == offset {
                break;
            }
            // o.leftOrigin < elt.leftOrigin
            if o_left_origin != elt.left
                && (o_left_origin.is_none()
                    || visited_id_spans
                        .iter()
                        .all(|x| !x.contains(o_left_origin.unwrap())))
            {
                break;
            }

            visited_id_spans.push(IdSpan::new(
                o_slice.elem.id.inc(offset as u32),
                end_offset - offset,
            ));

            if o_left_origin == elt.left {
                let o_right_origin = o_slice.elem.right;
                if o_right_origin == elt_right_origin {
                    if o_slice.elem.id.client > op.id.client {
                        break;
                    } else {
                        scanning = false;
                    }
                } else {
                    // We only need to compare the first element's right parent.
                    // And the first element's right parent is the same as the slice's right parent
                    // because they they share the rightOrigin
                    let o_right_cursor = o_slice.elem.right.map(|x| self.find_cursor(x));
                    let o_right_parent = o_right_cursor.and_then(|x| {
                        if self.find_left_origin(x) == elt_left_origin {
                            Some(x)
                        } else {
                            None
                        }
                    });

                    if elt_right_parent.is_none() {
                        let elt_right_cursor = elt.right.map(|x| self.find_cursor(x));
                        elt_right_parent = Some(elt_right_cursor.and_then(|x| {
                            if self.find_left_origin(x) == elt_left_origin {
                                Some(x)
                            } else {
                                None
                            }
                        }));
                    }

                    match self.cmp_right_parent_pos(o_right_parent, elt_right_parent.unwrap()) {
                        Ordering::Less => {
                            scanning = true;
                        }
                        Ordering::Equal if o_slice.elem.id.client > op.id.client => {
                            break;
                        }
                        _ => {
                            scanning = false;
                        }
                    }
                }
            }

            if !scanning {
                // set before to the last element
                let mut path = *o_slice.path();
                path.offset = end_offset - 1;
                left = Some(path);
            }
        }

        // convert left to right
        match left {
            Some(left) => Some(self.content.shift_path_by_one_offset(left)),
            None => Some(Some(scan_start)),
        }
    }

    #[inline]
    fn cmp_right_parent_pos(&self, a: Option<QueryResult>, b: Option<QueryResult>) -> Ordering {
        match (a, b) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => self.content.compare_pos(a, b),
        }
    }

    /// Merge data from other data into self
    pub fn merge(&mut self, other: &Self) {
        let vv = self.store.vv();
        let exported = other.export(&vv);
        let exported = decode(&exported);
        if cfg!(debug_assertions) || cfg!(feature = "test") {
            let expected = other.store.export(&vv);
            assert_eq!(exported, expected);
        }

        self.import_inner(exported);
    }

    fn import_inner(&mut self, exported: FxHashMap<ClientID, Vec<Op>>) {
        let mut all_ops = Vec::new();
        for (_, ops) in exported {
            for mut op in ops {
                let op = match self.store.can_apply(&op) {
                    op::CanApply::Yes => op,
                    op::CanApply::Trim(len) => {
                        op.slice_(len as usize..);
                        op
                    }
                    op::CanApply::Pending => {
                        self.pending_ops.push(op);
                        continue;
                    }
                    op::CanApply::Seen => {
                        continue;
                    }
                };
                self.store.insert(op.clone());
                all_ops.push(op);
            }
        }
        all_ops.sort_by(|a, b| a.lamport.cmp(&b.lamport));

        // Handling delete ops afterwards can guarantee the causal order.
        // Otherwise, the delete op may be applied before the insert op
        // because of the merges of delete ops.
        let mut deletions = Vec::new();
        for op in all_ops.iter() {
            if let OpContent::Del(_) = &op.content {
                deletions.push(op.clone());
            } else {
                self.apply(op.clone());
            }
        }

        for op in deletions {
            self.apply(op);
        }
    }

    pub fn version(&self) -> VersionVector {
        self.store.vv()
    }

    fn update_elem_in_id_range(
        &mut self,
        mut id: OpID,
        mut len: usize,
        mut f: impl FnMut(&mut Elem),
    ) {
        // debug_log::group!("update");
        // debug_log::debug_dbg!(id, len);
        // debug_log::debug_dbg!(&self.content);
        // debug_log::debug_dbg!(&self.cursor_map);
        // debug_log::group_end!();
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

    fn find_cursor(&self, id: OpID) -> QueryResult {
        // TODO: this method may use a hint to speed up
        let (insert_leaf, _) = self
            .cursor_map
            .get_insert(id)
            .expect("Cannot find target id");
        let node = self.content.get_node(insert_leaf);
        let mut elem_index = 0;
        let elements = &node.elements();
        while !elements[elem_index].contains_id(id) {
            // if range out of bound, then cursor_map is off
            elem_index += 1;
        }

        let offset = (id.counter - elements[elem_index].id.counter) as usize;
        assert!(offset < elements[elem_index].atom_len());
        QueryResult {
            leaf: insert_leaf,
            elem_index,
            offset,
            found: true,
        }
    }

    fn find_left_origin(&self, cursor: QueryResult) -> Option<OpID> {
        let offset = cursor.offset;
        let elem_index = cursor.elem_index;
        let node = self.content.get_node(cursor.leaf);
        let elements = node.elements();
        if offset == 0 {
            elements[elem_index].left
        } else {
            Some(elements[elem_index].id.inc(offset as u32 - 1))
        }
    }

    fn find_next_cursor_of(&self, id: Option<OpID>) -> Option<QueryResult> {
        match id {
            Some(id) => {
                let cursor = self.find_cursor(id);
                self.content.shift_path_by_one_offset(cursor)
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

    #[inline]
    #[allow(unused)]
    pub(crate) fn check(&self) {
        self.content.check();
    }

    pub fn debug_log(&self, include_content: bool) {
        debug_log::debug_log!("Text len = {} (utf16={})", self.len(), self.utf16_len());
        debug_log::debug_log!("Nodes len = {}", self.content.node_len());
        debug_log::debug_log!("Op len = {}", self.store.op_len());
        if include_content {
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
            debug_log::debug_log!("ContentTree = {}", content_inner);
            // println!("Text = {}", self);
            debug_log::debug_log!("Store = {:#?}", &self.store);
        }
    }

    pub fn check_no_mergeable_neighbor(&self) {
        let mut leaf_idx = Some(self.content.first_leaf());
        while let Some(leaf) = leaf_idx {
            let node = self.content.get_node(leaf);
            let elements = node.elements();
            for i in 0..elements.len() - 1 {
                if elements[i].can_merge(&elements[i + 1]) {
                    self.debug_log(false);
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

    pub fn get_line(&self, line: usize) -> Vec<Span> {
        let (start, finder) = self
            .content
            .query_with_finder_return::<LineStartFinder>(&line);
        if !start.found {
            return Vec::new();
        }

        let end = self.content.query::<LineStartFinder>(&(line + 1));
        let iter = iter::Iter::new_range(
            self,
            start,
            if end.found { Some(end) } else { None },
            finder.style_calculator,
        );

        iter.collect()
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
