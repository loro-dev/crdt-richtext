use std::{
    mem::replace,
    sync::{Arc, Mutex},
};

use generic_btree::{rle::HasLength, ArenaIndex, MoveEvent, MoveListener};

use crate::{Counter, OpID};

use super::{id_map::IdMap, rich_tree::Elem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cursor {
    Insert(ArenaIndex),
    // Delete(DeleteOp),
    // Ann(Arc<Annotation>),
}

#[derive(Debug)]
pub struct CursorMap {
    map: Arc<Mutex<IdMap<Cursor>>>,
}

impl CursorMap {
    pub fn new() -> Self {
        CursorMap {
            map: Arc::new(Mutex::new(IdMap::new())),
        }
    }

    pub fn gen_update_fn(&self) -> MoveListener<Elem> {
        let map = self.map.clone();
        Box::new(move |event| {
            listen(event, &mut map.try_lock().unwrap());
        })
    }

    #[inline]
    pub fn update(&mut self, event: MoveEvent<Elem>) {
        listen(event, &mut self.map.try_lock().unwrap());
    }

    // pub fn register_del(&mut self, op: &Op) {
    //     let mut map = self.map.try_lock().unwrap();
    //     let content = match &op.content {
    //         OpContent::Del(del) => del,
    //         _ => unreachable!(),
    //     };
    //     if let Some(mut start) = map.get_last(op.id) {
    //         if start.start_counter == op.id.counter {
    //             debug_assert!(op.rle_len() > start.len);
    //             let Cursor::Delete(del) = &mut start.value else { unreachable!() };
    //             debug_assert_eq!(del.start, content.start);
    //             del.len = content.len;
    //             start.len = op.rle_len();
    //             return;
    //         } else if start.start_counter + start.len as Counter == op.id.counter {
    //             if let Cursor::Delete(del) = &mut start.value {
    //                 if del.can_merge(content) {
    //                     del.merge_right(content);
    //                     start.len += content.rle_len();
    //                     return;
    //                 }
    //             }
    //         } else {
    //             // TODO: should we check here?
    //             return;
    //         }
    //     }

    //     map.insert(
    //         op.id,
    //         Cursor::Delete(*content),
    //         content.len.unsigned_abs() as usize,
    //     );
    // }

    // pub fn register_ann(&mut self, op: &Op) {
    //     let mut map = self.map.try_lock().unwrap();
    //     let content = match &op.content {
    //         OpContent::Ann(ann) => ann,
    //         _ => unreachable!(),
    //     };
    //     map.insert(op.id, Cursor::Ann(content.clone()), 1);
    // }

    pub fn get_insert(&self, id: OpID) -> Option<(ArenaIndex, usize)> {
        let map = self.map.try_lock().unwrap();
        if let Some(start) = map.get(id) {
            if start.start_counter <= id.counter
                && start.start_counter + start.len as Counter > id.counter
            {
                if let Cursor::Insert(leaf) = start.value {
                    return Some((
                        leaf,
                        start.len - (id.counter - start.start_counter) as usize,
                    ));
                } else {
                    unreachable!()
                }
            }
        }

        None
    }
}

fn listen(event: MoveEvent<Elem>, m: &mut IdMap<Cursor>) {
    let Some(leaf) = event.target_leaf else { return };
    let elem = event.elem;
    let mut id = elem.id;
    let mut cursor = Cursor::Insert(leaf);
    let mut len = elem.atom_len();
    'handle_old: {
        if let Some(nearest_last_span) = m.remove_range_return_last(elem.id, elem.atom_len()) {
            let mut nearest_last = nearest_last_span.borrow_mut();
            if nearest_last.start_counter + (nearest_last.len as Counter) <= elem.id.counter {
                // It have no overlap with the new element, break here
                break 'handle_old;
            }

            if nearest_last.value == Cursor::Insert(leaf) {
                // already has the same value as new elem
                if nearest_last.start_counter + (nearest_last.len as Counter)
                    < elem.id.counter + elem.atom_len() as Counter
                {
                    // extend the length if it's not enough
                    nearest_last.len =
                        (elem.id.counter - nearest_last.start_counter) as usize + elem.atom_len();
                }
                return;
            }

            if nearest_last.start_counter == elem.id.counter {
                // both have the same start counter
                if elem.rle_len() >= nearest_last.len {
                    // if new elem is longer, replace the target value
                    nearest_last.value = Cursor::Insert(leaf);
                    nearest_last.len = elem.atom_len();
                    return;
                } else {
                    // if new elem is shorter, split the last span:
                    //
                    // 1. set the new value and new len to the span
                    // 2. insert the rest of the last span to the map
                    let left_len = nearest_last.len - elem.atom_len();
                    let start_id = elem.id.inc(elem.atom_len() as Counter);
                    let old_value = replace(&mut nearest_last.value, Cursor::Insert(leaf));
                    nearest_last.len = elem.atom_len();
                    id = start_id;
                    cursor = old_value;
                    len = left_len;
                }
            } else {
                // remove the overlapped part from last span
                nearest_last.len = nearest_last
                    .len
                    .min((elem.id.counter - nearest_last.start_counter) as usize);
            }
        }
    }

    m.insert(id, cursor, len);
}

impl Default for CursorMap {
    fn default() -> Self {
        Self::new()
    }
}
