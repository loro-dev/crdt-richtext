use std::{fmt::Display, num::NonZeroU64, ops::RangeBounds};

use append_only_bytes::AppendOnlyBytes;

use generic_btree::{rle::HasLength, BTree};

use crate::{ClientID, Counter, Lamport, OpID, Style};

use self::{
    op::Op,
    rich_tree::{query::IndexFinder, rich_tree_btree_impl::RichTreeTrait, Elem},
    vv::VersionVector,
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
    vv: VersionVector,
    max_lamport: Lamport,
}

impl RichText {
    pub fn new(client_id: u64) -> Self {
        RichText {
            client_id: NonZeroU64::new(client_id).unwrap(),
            bytes: AppendOnlyBytes::new(),
            content: BTree::new(),
            vv: Default::default(),
            max_lamport: 0,
        }
    }

    fn next_id(&mut self) -> OpID {
        self.vv.use_next(self.client_id)
    }

    fn next_lamport(&mut self) -> Lamport {
        let temp = self.max_lamport;
        self.max_lamport += 1;
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
        let lamport = self.next_lamport();
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

        self.content.update_leaf(path.leaf, |elements| {
            if path.elem_index >= elements.len() {
                // insert at the end
                let mut left = None;
                if let Some(last) = elements.last_mut() {
                    if can_merge_new_slice(last, id, lamport, &slice) {
                        // can merge directly
                        last.string.try_merge(&slice).unwrap();
                        return true;
                    }
                    left = Some(last.id_last());
                }

                elements.push(Elem::new(id, left, lamport, slice));
                return true;
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
                if can_merge_new_slice(&elements[index], id, lamport, &slice) {
                    // can merge directly
                    elements[index].string.try_merge(&slice).unwrap();
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
            elements.splice(
                index + 1..index + 1,
                [
                    Elem::new(id, Some(elements[index].id_last()), lamport, slice),
                    right,
                ],
            );

            true
        });
    }

    pub fn delete(&mut self, range: impl RangeBounds<usize>) {
        todo!()
    }

    pub fn annotate(&mut self, range: impl RangeBounds<usize>, style: Style) {
        todo!()
    }

    pub fn apply(&mut self, ops: &[Op]) {
        todo!()
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

    pub fn utf16_len(&self) -> usize {
        self.content.root_cache().utf16_len
    }
}

impl Display for RichText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for span in self.content.iter() {
            f.write_str(std::str::from_utf8(&span.string).unwrap())?;
        }

        Ok(())
    }
}
