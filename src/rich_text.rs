use std::{
    fmt::Display,
    num::NonZeroU64,
    ops::{Bound, RangeBounds},
};

use append_only_bytes::AppendOnlyBytes;

use generic_btree::{rle::HasLength, BTree};
use smallvec::SmallVec;

use crate::{rich_text::op::OpContent, ClientID, Counter, Lamport, OpID, Style};

use self::{
    op::{Op, OpStore},
    rich_tree::{query::IndexFinder, rich_tree_btree_impl::RichTreeTrait, Elem},
};

mod id_map;
mod iter;
mod op;
mod rga;
mod rich_tree;
#[cfg(test)]
mod test;
mod vv;

pub struct RichText {
    client_id: ClientID,
    bytes: AppendOnlyBytes,
    content: BTree<RichTreeTrait>,
    store: OpStore,
    next_lamport: Lamport,
}

impl RichText {
    pub fn new(client_id: u64) -> Self {
        let client_id = NonZeroU64::new(client_id).unwrap();
        RichText {
            client_id,
            bytes: AppendOnlyBytes::new(),
            content: BTree::new(),
            store: OpStore::new(client_id),
            next_lamport: 0,
        }
    }

    fn next_id(&self) -> OpID {
        self.store.next_id()
    }

    fn next_lamport(&mut self, len: usize) -> Lamport {
        let temp = self.next_lamport;
        self.next_lamport += len as Lamport;
        temp
    }

    pub fn insert(&mut self, index: usize, string: &str) {
        fn can_merge_new_slice(
            elem: &Elem,
            id: OpID,
            lamport: u32,
            slice: &append_only_bytes::BytesSlice,
        ) -> bool {
            elem.start_id.client == id.client
                && elem.start_id.counter + elem.atom_len() as Counter == id.counter
                && elem.lamport + elem.atom_len() as Lamport == lamport
                && !elem.is_dead()
                && elem.string.can_merge(slice)
        }

        let start = self.bytes.len();
        self.bytes.push_str(string);
        let slice = self.bytes.slice(start..);
        let id = self.next_id();
        let lamport = self.next_lamport(slice.len());
        if index == 0 {
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
                        return true;
                    }
                    elements.push(Elem::new(id, left, lamport, slice));
                    return true;
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
                    return true;
                }

                elements.insert(
                    index + 1,
                    Elem::new(id, Some(elements[index].id_last()), lamport, slice),
                );
                return true;
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

            true
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
            elem.delete();
            deleted.push((elem.start_id, elem.rle_len()));
        };
        self.content
            .update(&start_result..&end_result, &mut |slice| {
                match (slice.start, slice.end) {
                    (Some((start_idx, start_offset)), Some((end_idx, end_offset)))
                        if start_idx == end_idx =>
                    {
                        // delete within one element
                        if start_idx >= slice.elements.len() {
                            return false;
                        }

                        let elem = &mut slice.elements[start_idx];
                        if elem.is_dead() {
                            return false;
                        }

                        let additions = elem.update(start_offset, end_offset, &mut delete_fn);
                        if !additions.is_empty() {
                            slice
                                .elements
                                .splice(start_idx + 1..start_idx + 1, additions);
                        }
                        return true;
                    }
                    _ => {}
                }

                let mut end = match slice.end {
                    Some((end_idx, end_offset)) => {
                        if end_offset == 0 {
                            end_idx
                        } else {
                            let elem = &mut slice.elements[end_idx];
                            if !elem.is_dead() {
                                let additions = elem.update(0, end_offset, &mut delete_fn);
                                if !additions.is_empty() {
                                    slice.elements.splice(end_idx + 1..end_idx + 1, additions);
                                }
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
                            if !elem.is_dead() {
                                let additions =
                                    elem.update(start_offset, elem.rle_len(), &mut delete_fn);
                                if !additions.is_empty() {
                                    end += additions.len();
                                    slice
                                        .elements
                                        .splice(start_idx + 1..start_idx + 1, additions);
                                }
                            }
                            start_idx + 1
                        }
                    }
                    None => 0,
                };

                for elem in slice.elements[start..end].iter_mut() {
                    delete_fn(elem);
                }

                true
            });

        for (start, len) in deleted {
            self.store
                .insert_local(OpContent::new_delete(start, len as i32));
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
    }

    pub fn iter(&self) -> impl Iterator<Item = &Elem> {
        self.content.iter()
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

    /// Merge data from other data into self
    pub fn merge(&mut self, other: &Self) {}
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
