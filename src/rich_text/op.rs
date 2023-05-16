use std::{ops::Deref, sync::Arc};

use append_only_bytes::BytesSlice;
use fxhash::FxHashMap;
use generic_btree::rle::{HasLength, Mergeable, Sliceable};

use crate::{Annotation, ClientID, Counter, Lamport, OpID};

use super::vv::VersionVector;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    pub id: OpID,
    pub lamport: Lamport,
    pub content: OpContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpContent {
    Ann(Arc<Annotation>),
    Text(TextInsertOp),
    Del(DeleteOp),
}

impl OpContent {
    pub fn new_insert(left: Option<OpID>, right: Option<OpID>, slice: BytesSlice) -> Self {
        OpContent::Text(TextInsertOp {
            text: slice,
            left,
            right,
        })
    }

    pub fn new_delete(mut start: OpID, mut len: i32) -> Self {
        if len > 0 {
            // prefer negative del
            start = start.inc_i32(len - 1);
            len = -len;
        }
        OpContent::Del(DeleteOp { start, len })
    }

    pub fn new_ann(ann: Arc<Annotation>) -> Self {
        OpContent::Ann(ann)
    }
}

#[derive(Clone)]
pub struct TextInsertOp {
    pub text: BytesSlice,
    pub left: Option<OpID>,
    pub right: Option<OpID>,
}

impl PartialEq for TextInsertOp {
    fn eq(&self, other: &Self) -> bool {
        self.text.deref() == other.text.deref()
            && self.left == other.left
            && self.right == other.right
    }
}

impl Eq for TextInsertOp {}

impl std::fmt::Debug for TextInsertOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInsertOp")
            .field("text", &std::str::from_utf8(&self.text))
            .field("left", &self.left)
            .field("right", &self.right)
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeleteOp {
    pub start: OpID,
    // can be negative, so we can merge backward
    pub len: i32,
}

impl HasLength for DeleteOp {
    fn rle_len(&self) -> usize {
        self.len.unsigned_abs() as usize
    }
}

impl PartialEq for DeleteOp {
    fn eq(&self, other: &Self) -> bool {
        if self.start.client != other.start.client {
            return false;
        }

        let p = if other.len > 0 {
            self.positive()
        } else {
            self.negative()
        };

        p.start.counter == other.start.counter && p.len == other.len
    }
}

impl Eq for DeleteOp {}

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

    fn next_counter(&self) -> (i32, Option<i32>) {
        (
            self.start.counter as i32 + self.len,
            if self.len.abs() == 1 {
                if self.len > 0 {
                    Some(self.start.counter as i32 - 1)
                } else {
                    Some(self.start.counter as i32 + 1)
                }
            } else {
                None
            },
        )
    }

    pub fn positive(&self) -> DeleteOp {
        if self.len > 0 {
            *self
        } else {
            DeleteOp {
                start: self.start.inc_i32(self.len + 1),
                len: -self.len,
            }
        }
    }

    pub fn negative(&self) -> DeleteOp {
        if self.len < 0 {
            *self
        } else {
            DeleteOp {
                start: self.start.inc_i32(self.len - 1),
                len: -self.len,
            }
        }
    }

    fn direction(&self) -> u8 {
        if self.len.abs() == 1 {
            0b11
        } else if self.len > 0 {
            0b01
        } else {
            0b10
        }
    }
}

impl Mergeable for DeleteOp {
    fn can_merge(&self, rhs: &Self) -> bool {
        if self.start.client != rhs.start.client || (self.direction() & rhs.direction()) == 0 {
            return false;
        }

        let (a, b) = self.next_counter();
        a == rhs.start.counter as i32 || b == Some(rhs.start.counter as i32)
    }

    fn merge_right(&mut self, rhs: &Self) {
        if self.len > 1 {
            self.len += rhs.len.abs();
        } else if self.len < -1 {
            self.len -= rhs.len.abs();
        } else if self.len.abs() == 1 {
            if rhs.start.counter > self.start.counter {
                self.len = rhs.len.abs() + 1;
            } else {
                self.len = -rhs.len.abs() - 1;
            }
        } else {
            unreachable!()
        }
    }

    fn merge_left(&mut self, left: &Self) {
        let mut left = *left;
        left.merge_right(self);
        *self = left;
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
                (OpContent::Text(left), OpContent::Text(right)) => {
                    right.left == Some(self.id.inc(self.rle_len() as Counter - 1))
                        && right.right == left.right
                        && left.text.can_merge(&right.text)
                }
                (OpContent::Del(a), OpContent::Del(b)) => a.can_merge(b),
                _ => false,
            }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (&mut self.content, &rhs.content) {
            (OpContent::Text(ins), OpContent::Text(ins2)) => {
                ins.text.try_merge(&ins2.text).unwrap();
            }
            (OpContent::Del(del), OpContent::Del(del2)) => del.merge_right(del2),
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, _left: &Self) {
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
                    right: if end == self.rle_len() {
                        text.right
                    } else {
                        Some(self.id.inc(end as Counter))
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

pub struct OpStore {
    map: FxHashMap<ClientID, Vec<Op>>,
    pub(crate) client: ClientID,
    next_lamport: Lamport,
}

impl std::fmt::Debug for OpStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpStore")
            .field("client", &self.client)
            .field("next_lamport", &self.next_lamport)
            .field("map", &self.map.len())
            .finish()?;

        for (key, value) in self.map.iter() {
            f.write_str("\n")?;
            f.write_str(&key.to_string())?;
            for op in value.iter() {
                f.write_str("\n    ")?;
                f.write_fmt(format_args!("{:?}", op))?;
            }
        }

        Ok(())
    }
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
        if op.lamport + op.rle_len() as Lamport >= self.next_lamport {
            self.next_lamport = op.lamport + op.rle_len() as Lamport;
        }
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
            vec.push(op);
            vec.last().as_ref().unwrap()
        }
    }

    pub fn export(&self, other_vv: &VersionVector) -> FxHashMap<ClientID, Vec<Op>> {
        let mut ans: FxHashMap<ClientID, Vec<Op>> = FxHashMap::default();
        for (client, vec) in self.map.iter() {
            let target_counter = other_vv.vv.get(client).unwrap_or(&0);
            if *target_counter
                >= vec
                    .last()
                    .map(|x| x.id.counter + x.rle_len() as Counter)
                    .unwrap_or(0)
            {
                continue;
            }

            let mut i = match vec.binary_search_by_key(target_counter, |op| op.id.counter) {
                Ok(i) => i,
                Err(i) => i.max(1) - 1,
            };
            if *target_counter >= vec[i].id.counter + vec[i].rle_len() as Counter {
                i += 1;
            }
            let vec = if vec[i].id.counter < *target_counter {
                let mut new_vec: Vec<Op> = Vec::with_capacity(vec.len() - i);
                new_vec.push(vec[i].slice(*target_counter as usize - vec[i].id.counter as usize..));
                new_vec.extend_from_slice(&vec[i + 1..]);
                new_vec
            } else {
                assert!(vec[i].id.counter == *target_counter);
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

    #[inline(always)]
    pub fn next_lamport(&self) -> u32 {
        self.next_lamport
    }

    pub fn op_len(&self) -> usize {
        self.map.iter().map(|x| x.1.len()).sum()
    }
}

pub enum CanApply {
    Yes,
    Trim(Counter),
    Pending,
    Seen,
}

#[cfg(test)]
mod test {
    use generic_btree::rle::Mergeable;

    use crate::OpID;

    use super::DeleteOp;

    #[test]
    fn del_merge() {
        let a = DeleteOp {
            start: OpID::new(1, 1),
            len: 2,
        };
        let b = DeleteOp {
            start: OpID::new(1, 3),
            len: -1,
        };
        assert!(a.can_merge(&b))
    }
}
