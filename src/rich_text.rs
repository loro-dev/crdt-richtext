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
use serde_json::Value;
use smallvec::SmallVec;

use crate::{
    rich_text::{
        ann::insert_anchors_at_same_elem,
        op::OpContent,
        rich_tree::utf16::{bytes_to_str, get_utf16_len_and_line_breaks, Utf16LenAndLineBreaks},
    },
    Anchor, AnchorType, Annotation, Behavior, ClientID, Counter, Expand, IdSpan, InternalString,
    OpID, Style,
};

use self::{
    ann::{insert_anchor_to_char, AnchorSetDiff, AnnIdx, AnnManager, StyleCalculator},
    cursor::CursorMap,
    delta::compose,
    encoding::{decode, encode},
    op::{Op, OpStore},
    rich_tree::{
        query::{IndexFinder, IndexFinderWithStyles, LineStartFinder},
        rich_tree_btree_impl::RichTreeTrait,
        utf16::{get_utf16_len, utf16_to_utf8},
        CacheDiff, Elem,
    },
    vv::VersionVector,
};

pub use ann::Span;
pub use delta::DeltaItem;
pub use error::Error;
pub use event::Event;
pub use rich_tree::query::IndexType;

mod ann;
mod cursor;
mod delta;
mod encoding;
mod error;
mod event;
mod id_map;
mod iter;
mod op;
mod rich_tree;
#[cfg(all(test, feature = "test"))]
mod test;
#[cfg(feature = "test")]
pub mod test_utils;
pub mod vv;

type Listener = Box<dyn FnMut(&Event)>;

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
    listeners: Vec<Listener>,
    event_index_type: IndexType,
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
            listeners: Vec::new(),
            event_index_type: IndexType::Utf8,
        }
    }

    pub fn id(&self) -> ClientID {
        self.store.client
    }

    pub fn set_event_index_type(&mut self, index_type: IndexType) {
        self.event_index_type = index_type;
    }

    pub fn observe(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    #[inline(always)]
    fn has_listener(&self) -> bool {
        !self.listeners.is_empty()
    }

    fn emit(&mut self, mut event: Event) {
        event.ops.retain(|x| !x.should_remove());
        for listener in &mut self.listeners {
            listener(&event);
        }
    }

    #[inline]
    fn next_id(&self) -> OpID {
        self.store.next_id()
    }

    #[inline]
    pub fn insert_utf16(&mut self, index: usize, string: &str) {
        assert!(index <= self.utf16_len());
        self.insert_inner(index, string, IndexType::Utf16);
    }

    #[inline]
    pub fn insert(&mut self, index: usize, string: &str) {
        assert!(index <= self.len());
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
        } else {
            // need to find left op id
            let path_to_right_origin = self.find_ideal_right_origin(index, index_type);
            let left;
            let right;
            let op_slice = slice.clone();
            {
                // find left and right
                let mut node = self.content.get_node(path_to_right_origin.leaf);
                let offset = path_to_right_origin.offset;
                let index = path_to_right_origin.elem_index;
                if offset != 0 {
                    left = Some(node.elements()[index].id.inc((offset - 1) as u32));
                } else {
                    left = Some(node.elements()[index - 1].id_last());
                }
                if offset < node.elements()[index].rle_len() {
                    right = Some(node.elements()[index].id.inc(offset as u32));
                } else if index + 1 < node.elements().len() {
                    right = Some(node.elements()[index + 1].id);
                } else if let Some(next) =
                    self.content.next_same_level_node(path_to_right_origin.leaf)
                {
                    node = self.content.get_node(next);
                    right = Some(node.elements()[0].id);
                } else {
                    right = None;
                }
            }

            self.content
                .update_leaf(path_to_right_origin.leaf, |elements| {
                    // insert new element
                    debug_assert!(path_to_right_origin.elem_index < elements.len());
                    let mut offset = path_to_right_origin.offset;
                    let mut index = path_to_right_origin.elem_index;
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
                            self.cursor_map.update(MoveEvent::new_move(
                                path_to_right_origin.leaf,
                                &elements[index],
                            ));
                            return (true, cache_diff);
                        }

                        elements.insert(index + 1, Elem::new(id, left, right, slice));
                        self.cursor_map.update(MoveEvent::new_move(
                            path_to_right_origin.leaf,
                            &elements[index + 1],
                        ));
                        return (true, cache_diff);
                    }

                    // need to split element
                    let right_half = elements[index].split(offset);
                    elements.splice(
                        index + 1..index + 1,
                        [Elem::new(id, left, right, slice), right_half],
                    );
                    self.cursor_map.update(MoveEvent::new_move(
                        path_to_right_origin.leaf,
                        &elements[index + 1],
                    ));
                    (true, cache_diff)
                });

            self.store
                .insert_local(OpContent::new_insert(left, right, op_slice));
        }

        if self.has_listener() {
            let retain = self.convert_index(index, index_type, self.event_index_type);
            let annotations = self
                .get_style_at_position(index, index_type)
                .map(|(k, v)| (k.to_string(), v))
                .collect();
            self.emit(Event {
                ops: vec![
                    DeltaItem::retain(retain),
                    DeltaItem::insert_with_attributes(
                        string.to_owned(),
                        self.event_index_type,
                        annotations,
                    ),
                ],
                is_local: true,
                index_type: self.event_index_type,
            })
        }
    }

    /// When user insert text at index, there may be tombstones at the given position.
    /// We need to find the ideal position among the tombstones to insert.
    /// Insertion at different position may have different styles.
    ///
    /// The ideal position is:
    ///
    /// 1. Before tombstones with new annotation
    /// 2. Before tombstones with before anchor
    /// 3. After tombstones with after anchor
    ///
    /// Sometimes the insertion may not have a ideal position for example an after anchor
    /// may exist before a before anchor.
    ///
    /// The current method will scan forward to find the last position that satisfies 1. and 2.
    /// Then it scans backward to find the first position that satisfies 3.
    ///
    /// The returned result points to the position the new insertion should be at.
    /// It uses the rle_len() rather than the content_len().
    ///
    /// - rle_len() includes the length of deleted string
    /// - content_len() does not include the length of deleted string
    fn find_ideal_right_origin(&mut self, index: usize, index_type: IndexType) -> QueryResult {
        assert!(index > 0);
        let mut path = self.content.query::<IndexFinder>(&(index - 1, index_type));
        // path may point to a tombstone now
        path = self.shift_to_next_char(path);
        'outer: loop {
            // scan forward to find the last position that satisfies 1. and 2.
            // after the loop, the path is the rightmost position that satisfies 1. and 2.
            let node = self.content.get_node(path.leaf);
            let elem = &node.elements()[path.elem_index];
            if !elem.is_dead() || elem.anchor_set.has_start_after() {
                break;
            }

            let mut new_path = path;
            new_path.elem_index += 1;
            new_path.offset = 0;
            while new_path.elem_index >= node.elements().len() {
                if let Some(next) = self.content.next_same_level_node(new_path.leaf) {
                    new_path.leaf = next;
                    new_path.elem_index = 0;
                } else {
                    break 'outer;
                }
            }
            let new_node = if new_path.leaf == path.leaf {
                node
            } else {
                self.content.get_node(new_path.leaf)
            };
            let new_elem = &new_node.elements()[new_path.elem_index];
            if new_elem.anchor_set.has_start_before() {
                break;
            } else {
                path = new_path;
            }
        }

        loop {
            // scan backward to find the first position that satisfies 3.
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

    /// Shift the path to the next char, including dead char
    ///
    /// NOTE that, the current path may point to the start byte
    /// of a char (which may take several bytes in fact)
    fn shift_to_next_char(&self, mut path: QueryResult) -> QueryResult {
        let mut node = self.content.get_node(path.leaf);
        let mut elem = &node.elements()[path.elem_index];
        let mut done = false;
        loop {
            while path.offset >= elem.rle_len() {
                let mut new_path = path;
                new_path.elem_index += 1;
                new_path.offset = 0;
                if new_path.elem_index >= node.elements().len() {
                    if let Some(next) = self.content.next_same_level_node(new_path.leaf) {
                        new_path.leaf = next;
                        new_path.elem_index = 0;
                    } else {
                        return path;
                    }
                }

                path = new_path;
                node = self.content.get_node(path.leaf);
                elem = &node.elements()[path.elem_index];
            }

            if done {
                break;
            }

            if !done {
                let char = bytes_to_str(&elem.string[path.offset..])
                    .chars()
                    .next()
                    .unwrap();
                path.offset += char.len_utf8();
                done = true;
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
            Bound::Unbounded => self.len_with(index_type),
        };

        if start == end {
            return;
        }

        assert!(end <= self.len_with(index_type));

        let event = if self.has_listener() {
            let retain = self.convert_index(start, index_type, self.event_index_type);
            let end = self.convert_index(end, index_type, self.event_index_type);
            Some(Event {
                ops: vec![DeltaItem::retain(retain), DeltaItem::delete(end - retain)],
                is_local: true,
                index_type: self.event_index_type,
            })
        } else {
            None
        };

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
                            slice
                                .elements
                                .splice(start_idx + 1..start_idx + 1, additions);
                            Elem::try_merge_arr(slice.elements, start_idx + 1);
                        }

                        Elem::try_merge_arr(slice.elements, start_idx);
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
                                line_break_diff += diff.unwrap().2;
                            }
                            start_idx + 1
                        }
                    }
                    None => 0,
                };

                for elem in slice.elements[start..end].iter_mut() {
                    if !elem.is_dead() {
                        let diff = delete_fn(elem);
                        len_diff += diff.0;
                        utf16_len_diff += diff.1;
                        line_break_diff += diff.2;
                    }
                }

                for i in start..end {
                    if i >= slice.elements.len() {
                        break;
                    }
                    Elem::try_merge_arr(slice.elements, i);
                }
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

        if let Some(event) = event {
            self.emit(event)
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

        let event = if self.has_listener() {
            let retain = self.convert_index(start, index_type, self.event_index_type);
            let end = self.convert_index(inclusive_end + 1, index_type, self.event_index_type);
            let mut attributes: FxHashMap<_, _> = Default::default();
            attributes.insert(style.type_.to_string(), style.value.clone());
            Some(Event {
                ops: vec![
                    DeltaItem::retain(retain),
                    DeltaItem::retain_with_attributes(end - retain, attributes),
                ],
                is_local: true,
                index_type: self.event_index_type,
            })
        } else {
            None
        };
        inclusive_end = inclusive_end.min(self.len_with(index_type) - 1);
        let start = if style.expand.start_type() == AnchorType::Before {
            Some(self.content.query::<IndexFinder>(&(start, index_type)))
        } else if start == 0 {
            None
        } else {
            Some(
                self.content
                    .query::<IndexFinder>(&(start.saturating_sub(1), index_type)),
            )
        };
        let inclusive_end = if style.expand.end_type() == AnchorType::Before {
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
                    type_: style.expand.start_type(),
                },
                end: Anchor {
                    id: end_id,
                    type_: style.expand.end_type(),
                },
            },
            behavior: style.behavior,
            type_: style.type_.clone(),
            value: style.value.clone(),
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
                        style.expand.start_type(),
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
                        style.end_type(),
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
        if let Some(event) = event {
            self.emit(event)
        }
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
                                style.end_type(),
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
                                style.start_type(),
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
                                style.start_type(),
                                style.end_type(),
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
                            style.end_type(),
                            false,
                        );
                        ann::insert_anchor_to_char(
                            elements,
                            start.elem_index,
                            start.offset,
                            ann_idx,
                            style.start_type(),
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

    fn apply(&mut self, op: Op) -> Vec<DeltaItem> {
        debug_log::group!("apply op");
        let mut ans = Vec::new();
        let has_listener = self.has_listener();
        'apply: {
            match &op.content {
                OpContent::Ann(ann) => {
                    let ann_idx = self.ann.register(ann.clone());
                    let mut start = 0;
                    match ann.range.start.id {
                        Some(start_id) => {
                            let cursor = self.find_cursor(start_id);
                            start = if has_listener {
                                self.get_index_from_path(cursor, self.event_index_type)
                            } else {
                                0
                            };
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
                            if has_listener {
                                ans.push(DeltaItem::retain(start));
                            }
                        }
                        None => {
                            self.init_styles.insert_start(ann_idx);
                        }
                    }

                    let mut end = self.len_with(self.event_index_type);
                    if let Some(end_id) = ann.range.end.id {
                        let cursor = self.find_cursor(end_id);
                        if has_listener {
                            end = self.get_index_from_path(cursor, self.event_index_type);
                        }
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
                    if has_listener {
                        let mut attributes: FxHashMap<_, _> = Default::default();
                        attributes.insert(ann.type_.to_string(), ann.value.clone());
                        ans.push(DeltaItem::retain_with_attributes(end - start, attributes));
                    }
                }
                OpContent::Text(text) => {
                    let right = match self.find_right(text, &op) {
                        Some(value) => value,
                        None => {
                            // insert to the last
                            let index = self.len_with(self.event_index_type);
                            self.content.push(Elem::new(
                                op.id,
                                text.left,
                                text.right,
                                text.text.clone(),
                            ));
                            if has_listener {
                                let annotations = self
                                    .get_style_at_position(index, self.event_index_type)
                                    .map(|(k, v)| (k.to_string(), v))
                                    .collect();
                                ans.push(DeltaItem::retain(index));
                                ans.push(DeltaItem::insert_with_attributes(
                                    bytes_to_str(&text.text).to_owned(),
                                    self.event_index_type,
                                    annotations,
                                ));
                            }
                            break 'apply;
                        }
                    };

                    let mut index = 0;
                    if let Some(right) = right {
                        if has_listener {
                            index = self.get_index_from_path(right, self.event_index_type);
                        }
                        self.content.insert_by_query_result(
                            right,
                            Elem::new(op.id, text.left, text.right, text.text.clone()),
                        );
                    } else {
                        if has_listener {
                            index = self.len_with(self.event_index_type);
                        }
                        self.content.push(Elem::new(
                            op.id,
                            text.left,
                            text.right,
                            text.text.clone(),
                        ));
                    }

                    if has_listener {
                        let annotations = self
                            .get_style_at_position(index, self.event_index_type)
                            .map(|(k, v)| (k.to_string(), v))
                            .collect();
                        ans.push(DeltaItem::retain(index));
                        ans.push(DeltaItem::insert_with_attributes(
                            bytes_to_str(&text.text).to_owned(),
                            self.event_index_type,
                            annotations,
                        ));
                    }
                }
                OpContent::Del(del) => {
                    let del = del.positive();
                    self.delete_in_id_range(del.start, del.len as usize, &mut ans)
                }
            }
        }

        debug_log::group_end!();
        ans
    }

    fn find_right(&mut self, elt: &op::TextInsertOp, op: &Op) -> Option<Option<QueryResult>> {
        // We use Fugue algorithm here, it has the property of "maximal non-interleaving"
        // See paper *The Art of the Fugue: Minimizing Interleaving in Collaborative Text Editing*
        let scan_start = self.find_next_cursor_of(elt.left)?;
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
        let mut delta = Vec::new();
        for op in all_ops.iter() {
            if let OpContent::Del(_) = &op.content {
                deletions.push(op.clone());
            } else {
                let new_delta = self.apply(op.clone());
                if self.has_listener() {
                    delta = compose(delta, new_delta);
                }
            }
        }

        for op in deletions {
            let new_delta = self.apply(op);
            if self.has_listener() {
                delta = compose(delta, new_delta);
            }
        }

        if self.has_listener() {
            self.emit(Event {
                ops: delta,
                is_local: false,
                index_type: self.event_index_type,
            })
        }
    }

    pub fn version(&self) -> VersionVector {
        self.store.vv()
    }

    fn delete_in_id_range(&mut self, mut id: OpID, mut len: usize, ans: &mut Vec<DeltaItem>) {
        // debug_log::group!("update");
        // debug_log::debug_dbg!(id, len);
        // debug_log::debug_dbg!(&self.content);
        // debug_log::debug_dbg!(&self.cursor_map);
        // debug_log::group_end!();
        let has_listener = self.has_listener();
        while len > 0 {
            let (insert_leaf, mut leaf_del_len) = self.cursor_map.get_insert(id).unwrap();
            leaf_del_len = leaf_del_len.min(len);
            // next record retain value
            let mut retain = if has_listener {
                self.get_index_from_path(
                    QueryResult {
                        leaf: insert_leaf,
                        elem_index: 0,
                        offset: 0,
                        found: true,
                    },
                    self.event_index_type,
                )
            } else {
                0
            };
            let leaf_del_len = leaf_del_len;
            let mut left_len = leaf_del_len;
            let mut new_delta = Vec::new();
            // Perf: we may optimize this by only update the cache once
            self.content.update_leaf(insert_leaf, |elements| {
                // dbg!(&elements, leaf_del_len);
                // there may be many pieces need to be updated inside one leaf node
                let mut index = 0;
                loop {
                    let elem = &elements[index];
                    if !elem.overlap(id, leaf_del_len) {
                        let len = elem.content_len_with(self.event_index_type);
                        retain += len;
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
                    if has_listener {
                        let start =
                            elements[index].slice_len_with(self.event_index_type, 0..offset);
                        retain += start;
                        let del_len =
                            elements[index].slice_len_with(self.event_index_type, offset..end);
                        let end = elements[index].slice_len_with(self.event_index_type, end..);
                        new_delta.push(DeltaItem::retain(retain));
                        new_delta.push(DeltaItem::delete(del_len));
                        retain = end;
                    }

                    let (new, _) =
                        elements[index].update(offset, end, &mut |elem| elem.apply_remote_delete());
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

            *ans = compose(ans.clone(), new_delta);
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
        if cfg!(debug_assertions) {
            let count = self.content.iter().count();
            debug_log::debug_log!("Elem len = {}", count);

            let mut count = 0;
            for (next, prev) in self.content.iter().skip(1).zip(self.content.iter()) {
                if prev.can_merge(next) {
                    count += 1;
                }
            }
            debug_log::debug_log!("Can be merged elems = {}", count);
        }

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

    pub fn slice_str(&self, range: impl RangeBounds<usize>, index_type: IndexType) -> String {
        let start = match range.start_bound() {
            Bound::Included(&start) => start,
            Bound::Excluded(&start) => start + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end + 1,
            Bound::Excluded(&end) => end,
            Bound::Unbounded => self.len_with(index_type),
        };

        let mut ans = String::with_capacity(end - start);
        let start = self.content.query::<IndexFinder>(&(start, index_type));
        let end = self.content.query::<IndexFinder>(&(end, index_type));
        for span in self.content.iter_range(start..end) {
            let s = &span.elem.string;
            ans.push_str(bytes_to_str(
                &s[span.start.unwrap_or(0)..span.end.unwrap_or(s.len())],
            ));
        }

        ans
    }

    pub fn slice(&self, range: impl RangeBounds<usize>, index_type: IndexType) -> Vec<Span> {
        let start = match range.start_bound() {
            Bound::Included(&start) => start,
            Bound::Excluded(&start) => start + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end + 1,
            Bound::Excluded(&end) => end,
            Bound::Unbounded => self.len_with(index_type),
        };

        let mut ans = Vec::new();
        let (start, finder) = self
            .content
            .query_with_finder_return::<IndexFinderWithStyles>(&(start, index_type));
        let style = finder.style_calculator;
        let end = self.content.query::<IndexFinder>(&(end, index_type));
        for span in iter::Iter::new_range(self, start, Some(end), style) {
            ans.push(span)
        }

        ans
    }

    pub fn get_style_at_position(
        &self,
        position: usize,
        index_type: IndexType,
    ) -> impl Iterator<Item = (InternalString, Value)> + '_ {
        let (_, finder) = self
            .content
            .query_with_finder_return::<IndexFinderWithStyles>(&(position, index_type));

        finder
            .style_calculator
            .calc_styles(&self.ann)
            .map(|x| (x.type_.clone(), x.value.clone()))
    }

    pub fn lines(&self) -> usize {
        self.content.root_cache().line_breaks as usize + 1
    }

    pub fn apply_delta(&mut self, delta: impl Iterator<Item = DeltaItem>, index_type: IndexType) {
        let mut index = 0;
        for delta_item in delta {
            match delta_item {
                DeltaItem::Retain { retain, attributes } => {
                    if let Some(attributes) = attributes {
                        let len = self.len_with(index_type);
                        // Quill assume there is always line break at the end of the text.
                        // But crdt-richtext doesn't have this assumption.
                        // This line break can be formatted by Quill, which might cause out of bound
                        // error. So we insert a line break if the delta is too short
                        if index + retain > len {
                            let new = index + retain - len;
                            self.insert(self.len(), &"\n".repeat(new));
                        }

                        for (key, value) in attributes {
                            let behavior = if value.is_null() {
                                crate::Behavior::Delete
                            } else {
                                crate::Behavior::Merge
                            };
                            self.annotate_inner(
                                index..index + retain,
                                Style::new_from_expand(
                                    if behavior == crate::Behavior::Delete {
                                        Expand::infer_delete_expand(&key)
                                    } else {
                                        Expand::infer_insert_expand(&key)
                                    },
                                    key.into(),
                                    value,
                                    behavior,
                                )
                                .unwrap(),
                                index_type,
                            )
                        }
                    }

                    index += retain;
                }
                DeltaItem::Insert {
                    insert, attributes, ..
                } => {
                    if insert.is_empty() {
                        continue;
                    }

                    self.insert_inner(index, &insert, index_type);
                    let end = match index_type {
                        IndexType::Utf8 => index + insert.len(),
                        IndexType::Utf16 => index + get_utf16_len(&insert),
                    };

                    let span = self
                        .slice(index..index + 1, index_type)
                        .into_iter()
                        .next()
                        .unwrap();
                    let inserted_attributes = span.attributes;
                    let attributes = attributes.unwrap_or_default();
                    for key in inserted_attributes.keys() {
                        if !attributes.contains_key(&key.to_string()) {
                            self.annotate_inner(
                                index..end,
                                Style::new_from_expand(
                                    Expand::infer_delete_expand(&key),
                                    key.into(),
                                    Value::Null,
                                    Behavior::Delete,
                                )
                                .unwrap(),
                                index_type,
                            )
                        }
                    }

                    for (key, value) in attributes {
                        let behavior = if value.is_null() {
                            crate::Behavior::Delete
                        } else {
                            crate::Behavior::Merge
                        };
                        if inserted_attributes.get(&key.as_str().into()) == Some(&value) {
                            continue;
                        }
                        self.annotate_inner(
                            index..end,
                            Style::new_from_expand(
                                Expand::infer_insert_expand(&key),
                                key.into(),
                                value,
                                behavior,
                            )
                            .unwrap(),
                            index_type,
                        )
                    }

                    index = end;
                }
                DeltaItem::Delete { delete } => {
                    self.delete_inner(index..index + delete, index_type);
                }
            }
        }
    }

    pub fn convert_index(&self, index: usize, from: IndexType, to: IndexType) -> usize {
        let path = self.content.query::<IndexFinder>(&(index, from));
        self.get_index_from_path(path, to)
    }

    fn get_index_from_path(&self, path: QueryResult, index_type: IndexType) -> usize {
        let mut count: usize = 0;
        self.content.visit_previous_caches(path, |v| match v {
            generic_btree::PreviousCache::NodeCache(cache) => {
                count += match index_type {
                    IndexType::Utf8 => cache.len,
                    IndexType::Utf16 => cache.utf16_len,
                } as usize;
            }
            generic_btree::PreviousCache::PrevSiblingElem(elem) => {
                if !elem.is_dead() {
                    count += match index_type {
                        IndexType::Utf8 => elem.content_len(),
                        IndexType::Utf16 => elem.utf16_len as usize,
                    };
                }
            }
            generic_btree::PreviousCache::ThisElemAndOffset { elem, offset } => {
                if !elem.is_dead() {
                    match index_type {
                        IndexType::Utf8 => count += utf16_to_utf8(&elem.string, offset),
                        IndexType::Utf16 => {
                            count += get_utf16_len_and_line_breaks(&elem.string[..offset]).utf16
                                as usize;
                        }
                    }
                }
            }
        });
        count
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
