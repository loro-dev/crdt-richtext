use std::mem::swap;

use fxhash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::rich_tree::{
    query::IndexType,
    utf16::{get_utf16_len, utf16_to_utf8},
};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum DeltaItem {
    Retain {
        retain: usize,
        attributes: Option<FxHashMap<String, Value>>,
    },
    Insert {
        insert: String,
        attributes: Option<FxHashMap<String, Value>>,
        len: Option<usize>,
        index_type: Option<IndexType>,
    },
    Delete {
        delete: usize,
    },
}

impl DeltaItem {
    pub fn retain(retain: usize) -> Self {
        Self::Retain {
            retain,
            attributes: None,
        }
    }

    pub fn insert(insert: String, index_type: IndexType) -> Self {
        Self::Insert {
            len: Some(match index_type {
                IndexType::Utf8 => insert.len(),
                IndexType::Utf16 => get_utf16_len(&insert),
            }),
            insert,
            index_type: Some(index_type),
            attributes: None,
        }
    }

    pub fn delete(delete: usize) -> Self {
        Self::Delete { delete }
    }

    pub fn retain_with_attributes(retain: usize, attributes: FxHashMap<String, Value>) -> Self {
        Self::Retain {
            retain,
            attributes: Some(attributes),
        }
    }

    pub fn insert_with_attributes(
        insert: String,
        index_type: IndexType,
        attributes: FxHashMap<String, Value>,
    ) -> Self {
        Self::Insert {
            len: Some(match index_type {
                IndexType::Utf8 => insert.len(),
                IndexType::Utf16 => get_utf16_len(&insert),
            }),
            insert,
            index_type: Some(index_type),
            attributes: Some(attributes),
        }
    }

    pub fn is_retain(&self) -> bool {
        matches!(self, Self::Retain { .. })
    }

    pub fn is_insert(&self) -> bool {
        matches!(self, Self::Insert { .. })
    }

    pub fn is_delete(&self) -> bool {
        matches!(self, Self::Delete { .. })
    }

    pub fn attributions(&self) -> Option<&FxHashMap<String, Value>> {
        match self {
            Self::Retain { attributes, .. } => attributes.as_ref(),
            Self::Insert { attributes, .. } => attributes.as_ref(),
            Self::Delete { .. } => None,
        }
    }

    pub fn length(&self) -> usize {
        match self {
            Self::Retain { retain, .. } => *retain,
            Self::Insert { len, insert, .. } => len.unwrap_or_else(|| get_utf16_len(insert)),
            Self::Delete { delete, .. } => *delete,
        }
    }

    pub fn should_remove(&self) -> bool {
        match self {
            Self::Retain { retain, .. } => *retain == 0,
            Self::Insert { .. } => false,
            Self::Delete { delete, .. } => *delete == 0,
        }
    }

    /// Take the first length characters from the delta item
    pub(crate) fn take(&mut self, length: usize) -> Self {
        match self {
            DeltaItem::Insert {
                insert,
                attributes,
                len,
                index_type,
            } => match index_type {
                Some(IndexType::Utf8) => {
                    let mut v = insert.split_off(length);
                    swap(&mut v, insert);
                    *len = Some(insert.len());

                    Self::Insert {
                        insert: v,
                        len: Some(length),
                        index_type: Some(IndexType::Utf8),
                        attributes: attributes.clone(),
                    }
                }
                None | Some(IndexType::Utf16) => {
                    let utf8length = utf16_to_utf8(insert.as_bytes(), length);
                    let mut v = insert.split_off(utf8length);
                    swap(&mut v, insert);
                    match len {
                        Some(len) => {
                            *len -= length;
                        }
                        None => *len = Some(get_utf16_len(&insert)),
                    }

                    Self::Insert {
                        insert: v,
                        len: Some(length),
                        index_type: *index_type,
                        attributes: attributes.clone(),
                    }
                }
            },
            DeltaItem::Retain { retain, attributes } => {
                *retain -= length;
                Self::Retain {
                    retain: length,
                    attributes: attributes.clone(),
                }
            }
            DeltaItem::Delete { delete } => {
                *delete -= length;
                Self::Delete { delete: length }
            }
        }
    }

    fn compose_meta(&mut self, next_op: &DeltaItem) {
        let attributions = match self {
            DeltaItem::Retain { attributes, .. } => attributes,
            DeltaItem::Insert { attributes, .. } => attributes,
            DeltaItem::Delete { .. } => return,
        };

        if attributions.is_none() {
            *attributions = Some(FxHashMap::default());
        }

        let self_attributions = attributions.as_mut().unwrap();
        if let Some(attributions) = next_op.attributions() {
            for attr in attributions {
                self_attributions.insert(attr.0.clone(), attr.1.clone());
            }
        }
    }
}

pub struct DeltaIterator {
    // The reversed Vec uses pop() to simulate getting the first element each time
    ops: Vec<DeltaItem>,
}

impl DeltaIterator {
    fn new(mut ops: Vec<DeltaItem>) -> Self {
        ops.reverse();
        Self { ops }
    }

    #[inline(always)]
    fn next<L: Into<Option<usize>>>(&mut self, len: L) -> DeltaItem {
        self.next_impl(len.into())
    }

    fn next_impl(&mut self, len: Option<usize>) -> DeltaItem {
        let length = len.unwrap_or(usize::MAX);
        let next_op = self.peek_mut();
        if next_op.is_none() {
            return DeltaItem::Retain {
                retain: usize::MAX,
                attributes: None,
            };
        }
        let op = next_op.unwrap();
        let op_length = op.length();
        if length < op_length {
            // a part of the peek op
            op.take(length)
        } else {
            self.take_peek().unwrap()
        }
    }

    fn next_with_ref(&mut self, len: usize, other: &DeltaItem) -> DeltaItem {
        let next_op = self.peek_mut();
        if next_op.is_none() {
            return DeltaItem::Retain {
                retain: other.length(),
                attributes: other.attributions().cloned(),
            };
        }
        let op = next_op.unwrap();
        let op_length = op.length();
        if len < op_length {
            // a part of the peek op
            op.take(len)
        } else {
            self.take_peek().unwrap()
        }
    }

    fn next_pair(&mut self, other: &mut Self) -> (DeltaItem, DeltaItem) {
        let self_len = self.peek_length();
        let other_len = other.peek_length();
        if self_len > other_len {
            let length = other_len;
            let other_op = other.next(None);
            debug_assert_eq!(other_op.length(), length);
            let this_op = self.next_with_ref(length, &other_op);
            (this_op, other_op)
        } else {
            let length = self_len;
            let this_op = self.next(None);
            debug_assert_eq!(this_op.length(), length);
            let other_op = other.next_with_ref(length, &this_op);
            (this_op, other_op)
        }
    }

    fn peek_mut(&mut self) -> Option<&mut DeltaItem> {
        self.ops.last_mut()
    }

    fn take_peek(&mut self) -> Option<DeltaItem> {
        self.ops.pop()
    }

    fn rest(mut self) -> Vec<DeltaItem> {
        self.ops.reverse();
        self.ops
    }

    fn has_next(&self) -> bool {
        !self.ops.is_empty()
    }

    fn peek(&self) -> Option<&DeltaItem> {
        self.ops.last()
    }

    fn peek_length(&self) -> usize {
        if let Some(op) = self.peek() {
            op.length()
        } else {
            usize::MAX
        }
    }

    fn peek_is_insert(&self) -> bool {
        if let Some(op) = self.peek() {
            op.is_insert()
        } else {
            false
        }
    }

    fn peek_is_delete(&self) -> bool {
        if let Some(op) = self.peek() {
            op.is_delete()
        } else {
            false
        }
    }
}

pub fn compose(delta_a: Vec<DeltaItem>, delta_b: Vec<DeltaItem>) -> Vec<DeltaItem> {
    let mut this_iter = DeltaIterator::new(delta_a);
    let mut other_iter = DeltaIterator::new(delta_b);
    let mut ops = vec![];
    let first_other = other_iter.peek();
    if let Some(first_other) = first_other {
        // if other.delta starts with retain, we insert corresponding number of inserts from self.delta
        if first_other.is_retain() && first_other.attributions().is_none() {
            let mut first_left = first_other.length();
            let mut first_this = this_iter.peek();
            while let Some(first_this_inner) = first_this {
                if first_this_inner.is_insert() && first_this_inner.length() <= first_left {
                    first_left -= first_this_inner.length();
                    let mut op = this_iter.next(None);
                    op.compose_meta(first_other);
                    ops.push(op);
                    first_this = this_iter.peek();
                } else {
                    break;
                }
            }
            if first_other.length() - first_left > 0 {
                other_iter.next(first_other.length() - first_left);
            }
        }
    }
    let mut delta = ops;
    while this_iter.has_next() || other_iter.has_next() {
        if other_iter.peek_is_insert() {
            // nothing to compose here
            delta.push(other_iter.next(None));
        } else if this_iter.peek_is_delete() {
            // nothing to compose here
            delta.push(this_iter.next(None));
        } else {
            // possible cases:
            // 1. this: insert, other: retain
            // 2. this: retain, other: retain
            // 3. this: retain, other: delete
            // 4. this: insert, other: delete

            let (mut this_op, mut other_op) = this_iter.next_pair(&mut other_iter);
            if other_op.is_retain() {
                // 1. this: insert, other: retain
                // 2. this: retain, other: retain
                this_op.compose_meta(&other_op);
                delta.push(this_op);
                let concat_rest = !other_iter.has_next();
                if concat_rest {
                    let vec = this_iter.rest();
                    if vec.is_empty() {
                        return chop(delta);
                    }
                    let mut rest = vec;
                    delta.append(&mut rest);
                    return chop(delta);
                }
            } else if other_op.is_delete() && this_op.is_retain() {
                // 3. this: retain, other: delete
                other_op.compose_meta(&this_op);
                // other deletes the retained text
                delta.push(other_op);
            } else {
                // 4. this: insert, other: delete
                // nothing to do here, because insert and delete have the same length
            }
        }
    }
    chop(delta)
}

fn chop(mut vec: Vec<DeltaItem>) -> Vec<DeltaItem> {
    let last_op = vec.last();
    if let Some(last_op) = last_op {
        if last_op.is_retain() && last_op.attributions().is_none() {
            vec.pop();
        }
    }

    vec
}
