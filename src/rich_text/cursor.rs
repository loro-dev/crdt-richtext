use std::{
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
                    return Some((leaf, start.len));
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
    if let Some(leaf) = event.target_leaf {
        let elem = &event.elem;
        if let Some(mut start) = m.get(elem.id) {
            if start.value == Cursor::Insert(leaf) {
                if start.start_counter + (start.len as Counter)
                    < elem.id.counter + elem.rle_len() as Counter
                {
                    start.len = (elem.id.counter - start.start_counter) as usize + elem.rle_len();
                }
                return;
            }

            if start.start_counter == elem.id.counter {
                start.value = Cursor::Insert(leaf);
                start.len = elem.rle_len();
                return;
            }

            start.len = (elem.id.counter - start.start_counter) as usize;
        }

        m.insert(elem.id, Cursor::Insert(leaf), elem.atom_len());
    }
}

impl Default for CursorMap {
    fn default() -> Self {
        Self::new()
    }
}
