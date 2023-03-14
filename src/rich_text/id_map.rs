use std::{
    cell::{RefCell, RefMut},
    collections::BTreeMap,
    rc::Rc,
};

use fxhash::FxHashMap;

use crate::{ClientID, Counter, OpID};

type Tree<T> = BTreeMap<Counter, Rc<RefCell<Entry<T>>>>;
/// This structure helps to map a range of IDs to a value.
///
/// It's the call site's responsibility to ensure there is no overlap in the range
#[derive(Debug, Default)]
pub(super) struct IdMap<Value> {
    pub(super) map: FxHashMap<ClientID, Tree<Value>>,
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

    #[allow(unused)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, id: OpID) -> Option<RefMut<'_, Entry<Value>>> {
        let client_map = self.map.get(&id.client)?;
        client_map
            .range(..=id.counter)
            .next_back()
            .and_then(|(counter, v)| {
                let v = v.borrow_mut();
                debug_assert_eq!(v.start_counter, *counter);
                if counter + v.len as Counter > id.counter {
                    Some(v)
                } else {
                    None
                }
            })
    }

    pub fn get_last(&self, id: OpID) -> Option<RefMut<'_, Entry<Value>>> {
        let client_map = self.map.get(&id.client)?;
        client_map
            .range(..=id.counter)
            .next_back()
            .map(|(counter, v)| {
                let v = v.borrow_mut();
                debug_assert_eq!(v.start_counter, *counter);
                v
            })
    }

    /// Remove any entries that start within the range of (exclusive_from, exclusive_from + len)
    ///
    /// It'll return the pointer to the alive last entry, which is the same as [`IdMap::get_last`]
    pub fn remove_range_return_last(
        &mut self,
        exclusive_from: OpID,
        len: usize,
    ) -> Option<Rc<RefCell<Entry<Value>>>> {
        let last_id = exclusive_from.inc((len - 1) as Counter);
        let Some(client_map) = self.map.get_mut(&last_id.client) else { return None };
        loop {
            let mutex_item = client_map
                .range(..=last_id.counter)
                .next_back()
                .map(|(_, v)| v);

            let item = mutex_item.map(|x| x.borrow_mut());
            let Some(item_inner) = item.as_ref() else { return None };
            let item_counter = item_inner.start_counter;
            let item_end = item_inner.len as Counter + item_counter;
            if item_inner.start_counter <= exclusive_from.counter {
                return mutex_item.cloned();
            }

            drop(item);
            let item = client_map.remove(&item_counter).unwrap();
            let new_item_counter = last_id.counter + 1;
            if item_end > new_item_counter {
                let mut inner = item.borrow_mut();
                inner.len = (item_end - new_item_counter) as usize;
                inner.start_counter = new_item_counter;
                drop(inner);
                client_map.insert(new_item_counter, item);
            }
        }
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
        let elem = Rc::new(RefCell::new(Entry {
            len,
            value: v,
            start_counter: id.counter,
        }));
        client_map.insert(id.counter, elem);
    }

    #[allow(unused)]
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
            let new_elem = Rc::new(RefCell::new(Entry {
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
