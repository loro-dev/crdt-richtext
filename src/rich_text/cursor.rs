use std::{
    mem::replace,
    ops::ControlFlow,
    sync::{Arc, Mutex},
};

use generic_btree::{rle::HasLength, ArenaIndex, MoveEvent, MoveListener};

use crate::{Counter, OpID};

use super::{
    id_map::IdMap,
    op::{Op, OpContent},
    rich_tree::Elem,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cursor {
    Insert(ArenaIndex),
    DeleteBackward(OpID),
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

    pub fn register_del(&mut self, op: &Op) {
        let mut map = self.map.try_lock().unwrap();
        let content = match &op.content {
            OpContent::Del(del) => del,
            _ => unreachable!(),
        };
        assert!(content.len < 0);
        if let Some(mut start) = map.get_last(op.id) {
            if start.start_counter == op.id.counter {
                debug_assert!(op.rle_len() > start.len);
                debug_assert_eq!(start.value, Cursor::DeleteBackward(content.start));
                start.len = op.rle_len();
                return;
            } else if start.start_counter + start.len as Counter == op.id.counter {
                if let Cursor::DeleteBackward(del) = start.value {
                    if del.inc_i32(-(start.len as i32)) == content.start {
                        start.len += (-content.len) as usize;
                        return;
                    }
                }
            } else {
                // TODO: should we check here?
                return;
            }
        }

        map.insert(
            op.id,
            Cursor::DeleteBackward(content.start),
            content.len.unsigned_abs() as usize,
        );
    }

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

    pub fn iter(&self) {
        todo!()
    }
}

fn listen(event: MoveEvent<Elem>, m: &mut IdMap<Cursor>) {
    let Some(leaf) = event.target_leaf else { return };
    let elem = event.elem;
    m.remove_range(elem.id, elem.atom_len());
    let mut id = elem.id;
    let mut cursor = Cursor::Insert(leaf);
    let mut len = elem.atom_len();
    if let Some(mut start) = m.get(elem.id) {
        if start.value == Cursor::Insert(leaf) {
            if start.start_counter + (start.len as Counter)
                < elem.id.counter + elem.atom_len() as Counter
            {
                start.len = (elem.id.counter - start.start_counter) as usize + elem.atom_len();
            }
            return;
        }

        if start.start_counter == elem.id.counter {
            if elem.rle_len() >= start.len {
                start.value = Cursor::Insert(leaf);
                start.len = elem.atom_len();
                return;
            } else {
                let left_len = start.len - elem.atom_len();
                let start_id = elem.id.inc(elem.atom_len() as Counter);
                let old_value = replace(&mut start.value, Cursor::Insert(leaf));
                start.len = elem.atom_len();
                id = start_id;
                cursor = old_value;
                len = left_len;
            }
        } else {
            start.len = start
                .len
                .min((elem.id.counter - start.start_counter) as usize);
        }
    }

    m.insert(id, cursor, len);
}

impl Default for CursorMap {
    fn default() -> Self {
        Self::new()
    }
}
