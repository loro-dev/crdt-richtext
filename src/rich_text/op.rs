use append_only_bytes::BytesSlice;
use fxhash::FxHashMap;
use generic_btree::rle::{HasLength, Mergeable, Sliceable};

use crate::{ClientID, Counter, Lamport, OpID, RangeOp};

use super::vv::VersionVector;

#[derive(Debug, Clone)]
pub struct Op {
    pub id: OpID,
    pub lamport: Lamport,
    pub content: OpContent,
}

impl OpContent {
    pub fn new_insert(left: Option<OpID>, slice: BytesSlice) -> Self {
        OpContent::Text(TextInsertOp { text: slice, left })
    }

    pub fn new_delete(mut start: OpID, mut len: i32) -> Self {
        if len > 0 {
            // prefer negative del
            start = start.inc_i32(len - 1);
            len = -len;
        }
        OpContent::Del(DeleteOp { start, len })
    }
}

#[derive(Debug, Clone)]
pub enum OpContent {
    Ann(RangeOp),
    Text(TextInsertOp),
    Del(DeleteOp),
}

#[derive(Debug, Clone)]
pub struct TextInsertOp {
    pub text: BytesSlice,
    pub left: Option<OpID>,
}

#[derive(Debug, Clone)]
pub struct DeleteOp {
    pub start: OpID,
    // can be negative, so we can merge backward
    pub len: i32,
}

impl DeleteOp {
    fn slice(&self, start: usize, end: usize) -> Self {
        let len = end - start;
        assert!(end <= self.len as usize);
        if self.len > 0 {
            Self {
                start: self.start.inc(start as Counter),
                len: len as i32,
            }
        } else {
            Self {
                start: self.start.inc_i32(-(start as i32)),
                len: -(len as i32),
            }
        }
    }

    pub fn next_id(&self) -> OpID {
        self.start.inc_i32(self.len)
    }

    pub fn positive_(&mut self) {
        if self.len > 0 {
            return;
        }

        self.start = self.start.inc_i32(self.len + 1);
        self.len = -self.len;
    }
}

impl HasLength for Op {
    fn rle_len(&self) -> usize {
        match &self.content {
            OpContent::Ann(_) => 1,
            OpContent::Text(text) => text.text.len(),
            OpContent::Del(del) => del.len.unsigned_abs() as usize,
        }
    }
}

impl Mergeable for Op {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.id.client == rhs.id.client
            && self.id.counter + self.rle_len() as Counter == rhs.id.counter
            && self.lamport + self.rle_len() as Counter == rhs.lamport
            && match (&self.content, &rhs.content) {
                (OpContent::Text(left), OpContent::Text(ins)) => {
                    ins.left == Some(self.id.inc(self.rle_len() as Counter - 1))
                        && left.text.can_merge(&ins.text)
                }
                (OpContent::Del(a), OpContent::Del(b)) => {
                    if a.start.client != b.start.client {
                        false
                    } else {
                        a.next_id().counter == b.start.counter
                        // TODO: +1/-1
                    }
                }
                _ => false,
            }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (&mut self.content, &rhs.content) {
            (OpContent::Text(ins), OpContent::Text(ins2)) => {
                ins.text.try_merge(&ins2.text).unwrap();
            }
            (OpContent::Del(del), OpContent::Del(del2)) => {
                del.len += del2.len;
            }
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, left: &Self) {
        unimplemented!()
    }
}

impl Sliceable for Op {
    fn slice(&self, range: impl std::ops::RangeBounds<usize>) -> Self {
        let start = match range.start_bound() {
            std::ops::Bound::Included(i) => *i,
            std::ops::Bound::Excluded(i) => *i + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(i) => *i + 1,
            std::ops::Bound::Excluded(i) => *i,
            std::ops::Bound::Unbounded => self.rle_len(),
        };
        match &self.content {
            OpContent::Ann(a) => Op {
                id: self.id.inc(start as Counter),
                lamport: self.lamport + (start as Lamport),
                content: OpContent::Ann(a.clone()),
            },
            OpContent::Text(text) => Op {
                id: self.id.inc(start as Counter),
                lamport: self.lamport + (start as Lamport),
                content: OpContent::Text(TextInsertOp {
                    text: text.text.slice_clone(start..end),
                    left: if start == 0 {
                        text.left
                    } else {
                        Some(self.id.inc(start as Counter - 1))
                    },
                }),
            },
            OpContent::Del(del) => Op {
                id: self.id.inc(start as Counter),
                lamport: self.lamport + (start as Lamport),
                content: OpContent::Del(del.slice(start, end)),
            },
        }
    }
}

#[derive(Debug)]
pub struct OpStore {
    map: FxHashMap<ClientID, Vec<Op>>,
    client: ClientID,
    next_lamport: Lamport,
}

impl OpStore {
    pub fn new(client: ClientID) -> Self {
        Self {
            map: Default::default(),
            client,
            next_lamport: 0,
        }
    }

    pub fn insert_local(&mut self, content: OpContent) -> &Op {
        let op = Op {
            id: self.next_id(),
            lamport: self.next_lamport,
            content,
        };
        self.next_lamport += op.rle_len() as Lamport;
        self.insert(op)
    }

    pub fn insert(&mut self, op: Op) -> &Op {
        let vec = self.map.entry(op.id.client).or_default();
        let mut done = false;
        if let Some(last) = vec.last_mut() {
            if last.can_merge(&op) {
                last.merge_right(&op);
                done = true;
            }
        }

        if done {
            vec.last().as_ref().unwrap()
        } else {
            if op.lamport + op.rle_len() as Lamport >= self.next_lamport {
                self.next_lamport = op.lamport + op.rle_len() as Lamport;
            }
            vec.push(op);
            vec.last().as_ref().unwrap()
        }
    }

    pub fn export(&self, other_vv: &VersionVector) -> FxHashMap<ClientID, Vec<Op>> {
        let mut ans: FxHashMap<ClientID, Vec<Op>> = FxHashMap::default();
        for (client, vec) in self.map.iter() {
            let counter = other_vv.vv.get(client).unwrap_or(&0);
            let i = match vec.binary_search_by_key(counter, |op| op.id.counter) {
                Ok(i) => i,
                Err(i) => i.max(1) - 1,
            };
            if i == vec.len() {
                continue;
            }
            let vec = if vec[i].id.counter < *counter {
                let mut new_vec: Vec<Op> = Vec::with_capacity(vec.len() - i);
                new_vec.push(vec[i].slice(*counter as usize - vec[i].id.counter as usize..));
                new_vec.extend_from_slice(&vec[i + 1..]);
                new_vec
            } else {
                assert!(vec[i].id.counter == *counter);
                vec[i..].to_vec()
            };
            ans.insert(*client, vec);
        }

        ans
    }

    pub fn vv(&self) -> VersionVector {
        let mut ans = VersionVector::default();
        for (client, vec) in self.map.iter() {
            if let Some(last) = vec.last() {
                ans.vv
                    .insert(*client, last.id.counter + last.rle_len() as Counter);
            }
        }

        ans
    }

    pub fn next_id(&self) -> OpID {
        OpID {
            client: self.client,
            counter: self
                .map
                .get(&self.client)
                .and_then(|v| v.last().map(|x| x.id.counter + x.rle_len() as Counter))
                .unwrap_or(0),
        }
    }

    pub fn can_apply(&self, op: &Op) -> CanApply {
        let Some(vec) = self.map.get(&op.id.client) else {
            if op.id.counter == 0 {
                return CanApply::Yes;
            } else {
                return CanApply::Pending;
            }
        };
        let end = vec
            .last()
            .map(|x| x.id.counter + x.rle_len() as Counter)
            .unwrap_or(0);
        if end == op.id.counter {
            return CanApply::Yes;
        }
        if end < op.id.counter {
            return CanApply::Pending;
        }
        if end >= op.id.counter + op.rle_len() as Counter {
            return CanApply::Seen;
        }

        CanApply::Trim(end - op.id.counter)
    }
}

pub enum CanApply {
    Yes,
    Trim(Counter),
    Pending,
    Seen,
}
