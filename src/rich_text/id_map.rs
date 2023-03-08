use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use fxhash::FxHashMap;

use crate::{ClientID, Counter, OpID};

/// This structure helps to map a range of IDs to a value.
///
/// It's the call site's responsibility to ensure there is no overlap in the range
#[derive(Debug)]
pub(super) struct IdMap<Value> {
    map: FxHashMap<ClientID, BTreeMap<Counter, Arc<Mutex<Elem<Value>>>>>,
    pending_changes: Mutex<Vec<Change<Value>>>,
}

#[derive(Debug)]
struct Change<Value> {
    id: ClientID,
    value: Option<Value>,
}

#[derive(Debug)]
pub(super) struct Elem<Value> {
    len: usize,
    start_counter: Counter,
    value: Value,
}

impl<Value: Clone> IdMap<Value> {
    pub fn new() -> Self {
        Self {
            map: Default::default(),
            pending_changes: Default::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, id: OpID) -> Option<&Arc<Mutex<Elem<Value>>>> {
        let client_map = self.map.get(&id.client)?;
        client_map
            .range(..=id.counter)
            .next_back()
            .and_then(|(counter, v)| {
                if counter + v.try_lock().unwrap().len as Counter >= id.counter {
                    Some(v)
                } else {
                    None
                }
            })
    }

    pub fn insert(&mut self, id: OpID, v: Value, len: usize) {
        debug_assert!(self.get(id).is_none());
        let client_map = self.map.entry(id.client).or_default();
        let elem = Arc::new(Mutex::new(Elem {
            len,
            value: v,
            start_counter: id.counter,
        }));
        client_map.insert(id.counter, elem);
    }

    pub fn remove(&mut self, id: OpID, len: usize) -> bool {
        let Some(entry) = self.get(id) else { return false };
        let mut g = entry.try_lock().unwrap();
        if g.start_counter == id.counter && g.len == len {
            // remove entry directly
            drop(g);
            let client_map = self.map.get_mut(&id.client).unwrap();
            client_map.remove(&id.counter);
        } else if g.start_counter == id.counter {
            // split entry
            g.start_counter += len as Counter;
            g.len -= len;
            drop(g);
            let client_map = self.map.get_mut(&id.client).unwrap();
            let Some((_, value)) = client_map.remove_entry(&id.counter) else {unreachable!()};
            client_map.insert(id.counter + len as Counter, value);
        } else if g.start_counter + g.len as Counter == id.counter + len as Counter {
            // adjust length
            g.len -= len;
        } else {
            // adjust length + split
            let start_counter = id.counter + len as Counter;
            let new_elem = Arc::new(Mutex::new(Elem {
                len: g.len - len - (id.counter - g.start_counter) as usize,
                value: g.value.clone(),
                start_counter,
            }));
            g.len -= len;
            drop(g);
            let client_map = self.map.get_mut(&id.client).unwrap();
            client_map.insert(start_counter, new_elem);
        }
        true
    }
}
