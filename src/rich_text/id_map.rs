use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
};

use fxhash::FxHashMap;

use crate::{ClientID, Counter, OpID};

type Tree<T> = BTreeMap<Counter, Arc<Mutex<Entry<T>>>>;
/// This structure helps to map a range of IDs to a value.
///
/// It's the call site's responsibility to ensure there is no overlap in the range
#[derive(Debug, Default)]
pub(super) struct IdMap<Value> {
    pub(super) map: FxHashMap<ClientID, Tree<Value>>,
}

#[derive(Debug)]
struct Change<Value> {
    id: ClientID,
    value: Option<Value>,
}

#[derive(Debug)]
pub(super) struct Entry<Value> {
    pub len: usize,
    pub start_counter: Counter,
    pub value: Value,
}

impl<Value: Clone + std::fmt::Debug> IdMap<Value> {
    pub fn new() -> Self {
        Self {
            map: Default::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, id: OpID) -> Option<MutexGuard<'_, Entry<Value>>> {
        let client_map = self.map.get(&id.client)?;
        client_map
            .range(..=id.counter)
            .next_back()
            .and_then(|(counter, v)| {
                let v = v.try_lock().unwrap();
                debug_assert_eq!(v.start_counter, *counter);
                if counter + v.len as Counter > id.counter {
                    Some(v)
                } else {
                    None
                }
            })
    }

    pub fn get_last(&self, id: OpID) -> Option<MutexGuard<'_, Entry<Value>>> {
        let client_map = self.map.get(&id.client)?;
        client_map
            .range(..=id.counter)
            .next_back()
            .map(|(counter, v)| {
                let v = v.try_lock().unwrap();
                debug_assert_eq!(v.start_counter, *counter);
                v
            })
    }

    pub fn insert(&mut self, id: OpID, v: Value, len: usize) {
        debug_assert!(
            self.get(id).is_none(),
            "Unexpected overlap {:?} {:?} {:#?}",
            id,
            &v,
            self.get(id).unwrap()
        );
        let client_map = self.map.entry(id.client).or_default();
        let elem = Arc::new(Mutex::new(Entry {
            len,
            value: v,
            start_counter: id.counter,
        }));
        client_map.insert(id.counter, elem);
    }

    pub fn remove(&mut self, id: OpID, len: usize) -> bool {
        let Some(mut g) = self.get(id) else { return false };
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
            let new_elem = Arc::new(Mutex::new(Entry {
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
