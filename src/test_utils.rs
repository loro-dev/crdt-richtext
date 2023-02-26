use std::collections::{BTreeSet, HashSet};

use crate::range_map::tree_impl::TreeRangeMap;

use super::*;
use arbitrary::Arbitrary;
use crdt_list::crdt::ListCrdt;
use crdt_list::test::TestFramework;
use crdt_list::yata::{self};
use crdt_list::yata_dumb_impl::{Container, Op, OpId as ListOpId, YataImpl};

#[derive(Debug, PartialEq, Eq)]
pub struct SimpleSpan {
    pub len: usize,
    pub annotations: HashSet<String>,
}

impl From<&Span> for SimpleSpan {
    fn from(value: &Span) -> Self {
        Self {
            len: value.len,
            annotations: value
                .annotations
                .iter()
                .filter_map(|x| {
                    if x.merge_method == RangeMergeRule::Delete {
                        None
                    } else {
                        Some(x.type_.clone())
                    }
                })
                .collect(),
        }
    }
}

pub struct Actor {
    list: Container,
    range: CrdtRange<TreeRangeMap>,
    visited: HashSet<OpID>,
    list_ops: Vec<Op>,
    range_ops: Vec<RangeOp>,
    deleted: HashSet<ListOpId>,
    next_lamport: Lamport,
    len: usize,
}

#[derive(Arbitrary, Clone, Copy, Debug)]
pub enum AnnotationType {
    Link,
    Bold,
    Comment,
    UnBold,
    UnLink,
}

#[derive(Arbitrary, Clone, Debug, Copy)]
pub enum Action {
    Insert {
        actor: u8,
        pos: u8,
        len: u8,
    },
    Delete {
        actor: u8,
        pos: u8,
        len: u8,
    },
    Annotate {
        actor: u8,
        pos: u8,
        len: u8,
        annotation: AnnotationType,
    },
    Sync(u8, u8),
}

impl From<ListOpId> for OpID {
    fn from(value: ListOpId) -> Self {
        OpID {
            client: value.client_id as ClientID,
            counter: value.clock as Counter,
        }
    }
}

/// return text_index, arr_index
fn index(list: &Container, target_id: OpID) -> (Result<usize, usize>, usize) {
    let mut text_index = 0;
    let mut arr_index = 0;
    let mut found = false;
    let mut deleted = false;
    for op in list.content.iter() {
        let id: ListOpId = op.id;
        if OpID::from(id) == target_id {
            found = true;
            deleted = op.deleted;
            break;
        }
        if !op.deleted {
            text_index += 1;
        }
        arr_index += 1;
    }

    if !found {
        panic!("target not found");
    }

    (
        if deleted {
            Err(text_index)
        } else {
            Ok(text_index)
        },
        arr_index,
    )
}

pub fn preprocess_action(actors: &[Actor], action: &mut Action) {
    match action {
        Action::Insert { actor, pos, len } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len + 1)) as u8;
            *len = (*len).min(10);
            *len = (*len).min(255).max(1);
        }
        Action::Delete { actor, pos, len } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len + 1)) as u8;
            *len = (*len).min(10);
            *len %= (actors[*actor as usize].len.max(*pos as usize + 1) - *pos as usize)
                .min(255)
                .max(1) as u8;
        }
        Action::Annotate {
            actor,
            pos,
            len,
            annotation: _,
        } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len + 1)) as u8;
            *len = (*len).min(10);
            *len %= (actors[*actor as usize].len.max(*pos as usize + 1) - *pos as usize)
                .min(255)
                .max(1) as u8;
        }
        Action::Sync(a, b) => {
            *a %= actors.len() as u8;
            *b %= actors.len() as u8;
            if b == a {
                *b = (*a + 1) % actors.len() as u8;
            }
        }
    }
}

pub fn apply_action(actors: &mut [Actor], action: Action) {
    match action {
        Action::Insert { actor, pos, len } => {
            if len == 0 {
                return;
            }
            actors[actor as usize].insert(pos as usize, len as usize);
            actors[actor as usize].check();
        }
        Action::Delete { actor, pos, len } => {
            if len == 0 {
                return;
            }

            actors[actor as usize].delete(pos as usize, len as usize);
            actors[actor as usize].check();
        }
        Action::Annotate {
            actor,
            pos,
            len,
            annotation,
        } => {
            if len == 0 {
                return;
            }

            match annotation {
                AnnotationType::Link => {
                    actors[actor as usize]
                        .annotate(pos as usize..=pos as usize + len as usize - 1, "link");
                }
                AnnotationType::Bold => {
                    actors[actor as usize]
                        .annotate(pos as usize..pos as usize + len as usize, "bold");
                }
                AnnotationType::Comment => {
                    // TODO:
                }
                AnnotationType::UnBold => {
                    actors[actor as usize]
                        .un_annotate(pos as usize..pos as usize + len as usize, "bold");
                }
                AnnotationType::UnLink => {
                    actors[actor as usize]
                        .un_annotate(pos as usize..=pos as usize + len as usize - 1, "link");
                }
            }
            actors[actor as usize].check();
        }
        Action::Sync(a, b) => {
            let (a, b) = arref::array_mut_ref!(actors, [a as usize, b as usize]);
            a.merge(b);
            a.check();
        }
    }
}

pub fn fuzzing(actor_num: usize, actions: Vec<Action>) {
    let mut actors = vec![];
    for i in 0..actor_num {
        actors.push(Actor::new(i));
    }

    for mut action in actions {
        preprocess_action(&actors, &mut action);
        // println!("{:?},", &action);
        debug_log::group!("{:?},", &action);
        apply_action(&mut actors, action);
        debug_log::group_end!();
    }

    for i in 0..actors.len() {
        for j in (i + 1)..actors.len() {
            let (a, b) = arref::array_mut_ref!(&mut actors, [i, j]);
            a.check();
            b.check();
            println!("merge {i}<-{j}");
            debug_log::group!("merge {i}<-{j}");
            a.merge(b);
            debug_log::group_end!();
            println!("merge {j}<-{i}");
            debug_log::group!("merge {i}->{j}");
            b.merge(a);
            debug_log::group_end!();
            let _patches = a
                .range_ops
                .iter()
                .filter(|x| match x {
                    RangeOp::Patch(_) => true,
                    RangeOp::Annotate(_) => false,
                })
                .collect::<Vec<_>>();
            assert_eq!(&*a.list.content, &*b.list.content);
            // dbg!(a
            //     .list
            //     .content
            //     .iter()
            //     .enumerate()
            //     .map(|(i, x)| format!("{}:{}-{}", i, x.id.client_id, x.id.clock))
            //     .collect::<Vec<_>>()
            //     .join(", "));
            // dbg!(&patches);
            // dbg!(&a.range);
            // dbg!(&a.get_annotations(..));
            assert_eq!(a.get_annotations(..), b.get_annotations(..));
        }
    }
}

impl Actor {
    pub fn new(id: usize) -> Self {
        Self {
            deleted: HashSet::new(),
            len: 0,
            list: YataImpl::new_container(id),
            list_ops: vec![],
            next_lamport: 0,
            range: CrdtRange::new(),
            range_ops: vec![],
            visited: HashSet::new(),
        }
    }

    pub fn insert(&mut self, pos: usize, len: usize) {
        debug_log::group!("insert");
        let (arr_pos, op) = self._get_list_insert_op(pos);

        self.integrate_insert_op(&op, true);
        self.visited.insert(op.id.into());
        self.list_ops.push(op.clone());
        for i in 1..len {
            let (_, op) = self._get_list_insert_op(pos + i);
            self.integrate_insert_op(&op, true);
            self.visited.insert(op.id.into());
            self.list_ops.push(op);
        }

        debug_log::debug_dbg!(&self.range);
        self.next_lamport += len as Lamport;
        self._range_insert(len, &op, arr_pos, true);
        debug_log::group_end!();
    }

    /// this should happen after the op is integrated to the list crdt
    fn _range_insert(&mut self, len: usize, first_op: &Op, arr_pos: usize, is_local: bool) {
        let right = {
            let mut next_alive_arr_index = arr_pos + len;
            while self
                .list
                .content
                .get(next_alive_arr_index)
                .map(|x| x.deleted)
                .unwrap_or(false)
            {
                next_alive_arr_index += 1;
            }

            self.list
                .content
                .get(next_alive_arr_index)
                .map(|x| x.id.into())
        };
        let left = if arr_pos != 0 {
            let mut last_alive_arr_index = arr_pos - 1;
            while self
                .list
                .content
                .get(last_alive_arr_index)
                .map(|x| x.deleted)
                .unwrap_or(false)
            {
                if last_alive_arr_index == 0 {
                    break;
                }

                last_alive_arr_index -= 1;
            }

            self.list.content.get(last_alive_arr_index).and_then(|x| {
                if x.deleted {
                    assert_eq!(last_alive_arr_index, 0);
                    None
                } else {
                    Some(x.id.into())
                }
            })
        } else {
            None
        };

        let new_op_id = first_op.id;
        let mut left_set: BTreeSet<OpID> = BTreeSet::new();
        let mut i = 0;
        let mut text_pos = 0;
        while self.list.content[i].id != new_op_id {
            left_set.insert(self.list.content[i].id.into());
            if self.list.content.get(i).map(|x| x.deleted).unwrap_or(false) {
                i += 1;
                continue;
            }

            text_pos += 1;
            i += 1;
        }

        assert_eq!(i, arr_pos);
        let range_ops = self.range.insert_text(
            text_pos,
            len,
            is_local,
            left,
            right,
            self.next_lamport,
            self.next_id(),
            |a| {
                // this can be O(lgN) using a proper data structure
                if left_set.contains(&a) {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            },
        );

        if !range_ops.is_empty() {
            debug_log::debug_log!("range_ops: {:#?}", range_ops);
        }
        self.list.max_clock += range_ops.len();
        self.next_lamport += range_ops.len() as Lamport;
        for op in range_ops.iter() {
            self.visited.insert(op.id());
        }

        self.range_ops.extend(range_ops);
    }

    pub fn delete(&mut self, pos: usize, len: usize) {
        self.len -= len;
        let op = YataImpl::new_del_op(&self.list, pos, len);
        YataImpl::integrate_delete_op(&mut self.list, op.clone());
        self.deleted.extend(op.into_iter());
        self.next_lamport += len as Lamport;
        self.range.delete_text(pos, len);
    }

    #[inline(always)]
    pub fn annotate(&mut self, range: impl RangeBounds<usize>, type_: &str) {
        self.annotate_with_type(range, type_, RangeMergeRule::Merge);
    }

    #[inline(always)]
    fn un_annotate(&mut self, range: impl RangeBounds<usize>, type_: &str) {
        self.annotate_with_type(range, type_, RangeMergeRule::Delete);
    }

    fn annotate_with_type(
        &mut self,
        range: impl RangeBounds<usize>,
        type_: &str,
        merge_method: RangeMergeRule,
    ) {
        let id = self._use_next_id();
        let lamport = self._use_next_lamport();
        let start = match range.start_bound() {
            Bound::Included(x) => self
                ._list_op_id_at_real_index(*x)
                .map_or(Anchor::after_none(), Anchor::before),
            std::ops::Bound::Excluded(x) => self
                ._list_op_id_at_real_index(*x)
                .map_or(Anchor::after_none(), Anchor::after),
            std::ops::Bound::Unbounded => Anchor::after_none(),
        };
        let end = match range.end_bound() {
            Bound::Included(x) => self
                ._list_op_id_at_real_index(*x)
                .map_or(Anchor::before_none(), Anchor::after),
            std::ops::Bound::Excluded(x) => self
                ._list_op_id_at_real_index(*x)
                .map_or(Anchor::before_none(), Anchor::before),
            std::ops::Bound::Unbounded => Anchor::before_none(),
        };
        self.visited.insert(id);
        let ann = Annotation {
            id,
            range_lamport: (lamport, id),
            range: AnchorRange { start, end },
            merge_method,
            type_: type_.to_string(),
            meta: None,
        };
        debug_log::debug_dbg!(&ann);
        self.range_ops.push(self.range.annotate(ann, range));
    }

    pub fn get_annotations(&mut self, range: impl RangeBounds<usize>) -> Vec<SimpleSpan> {
        let mut spans = vec![];
        for span in self
            .range
            .get_annotations(range)
            .iter()
            .map(|x| -> SimpleSpan { x.into() })
        {
            if span.len == 0 {
                continue;
            }
            if spans
                .last()
                .map(|x: &SimpleSpan| x.annotations == span.annotations)
                .unwrap_or(false)
            {
                spans.last_mut().unwrap().len += span.len;
            } else {
                spans.push(span);
            }
        }

        spans
    }

    pub fn delete_annotation(&mut self, id: OpID) {
        let lamport = self._use_next_lamport();
        let op_id = self._use_next_id();
        self.range_ops
            .push(self.range.delete_annotation(lamport, op_id, id));
    }

    fn next_id(&self) -> OpID {
        OpID {
            client: self.list.id as ClientID,
            counter: self.list.max_clock as Counter,
        }
    }

    fn _use_next_id(&mut self) -> OpID {
        let id = OpID {
            client: self.list.id as ClientID,
            counter: self.list.max_clock as Counter,
        };
        self.list.max_clock += 1;
        id
    }

    fn _use_next_lamport(&mut self) -> Lamport {
        self.next_lamport += 1;
        self.next_lamport - 1
    }

    fn _list_op_id_at_real_index(&self, pos: usize) -> Option<OpID> {
        let list: &Container = &self.list;
        let insert_pos = if pos == self.len {
            list.content.len()
        } else {
            list.content.real_index(pos)
        };

        list.content.get(insert_pos).map(|x| x.id.into())
    }

    fn _get_list_insert_op(&mut self, pos: usize) -> (usize, Op) {
        let container: &mut Container = &mut self.list;
        let insert_pos = get_insert_pos(pos, container);
        let op = {
            let (left, right) = (
                if insert_pos >= 1 {
                    container.content.get(insert_pos - 1).map(|x| x.id)
                } else {
                    None
                },
                container.content.get(insert_pos).map(|x| x.id),
            );

            let ans = Op {
                id: ListOpId {
                    client_id: container.id,
                    clock: container.max_clock,
                },
                left,
                right,
                deleted: false,
            };

            container.max_clock += 1;
            ans
        };
        (insert_pos, op)
    }

    fn merge(&mut self, other: &Self) {
        debug_log::debug_dbg!(&self.list.id);
        assert_ne!(self.list.id, other.list.id);
        // insert text
        for op in other.list_ops.iter() {
            let id = op.id.into();
            if !self.visited.contains(&id) {
                self.integrate_insert_op(op, false);
                self.list_ops.push(op.clone());
                self.visited.insert(op.id.into());
            }
        }

        {
            // delete text
            let mut new_deleted: HashSet<ListOpId> = HashSet::new();
            for id in other.deleted.iter() {
                if !self.deleted.contains(id) {
                    new_deleted.insert(*id);
                    self.deleted.insert(*id);
                    self.len -= 1;
                }
            }

            let mut deleted_text: Vec<(usize, usize)> = vec![];
            let container = &mut self.list;
            for (text_index, op) in container.content.iter_real_mut().enumerate() {
                if new_deleted.contains(&op.id) {
                    op.deleted = true;
                    if let Some(last) = deleted_text.last_mut() {
                        if last.0 + last.1 == text_index {
                            last.1 += 1;
                            continue;
                        }
                    }

                    deleted_text.push((text_index, 1));
                }
            }
            for (index, len) in deleted_text.iter().rev() {
                self.range.delete_text(*index, *len);
            }
        }

        debug_log::debug_dbg!(self
            .list
            .content
            .iter()
            .enumerate()
            .map(|(i, x)| format!("{}:{}-{}", i, x.id.client_id, x.id.clock))
            .collect::<Vec<_>>()
            .join(", "));
        // annotation
        debug_log::group!("apply remote annotation");
        for op in other.range_ops.iter() {
            if !self.visited.contains(&op.id()) {
                debug_log::group!("apply {:?}", &op);
                self.range
                    .apply_remote_op(op.clone(), &|x| index(&self.list, x).0);
                self.range_ops.push(op.clone());
                self.visited.insert(op.id());
                debug_log::group_end!();
            }
        }
        debug_log::group_end!();

        // lamport
        self.next_lamport = std::cmp::max(self.next_lamport, other.next_lamport);
        self.check();
    }

    fn integrate_insert_op(&mut self, op: &Op, is_local: bool) {
        assert!(!self.visited.contains(&op.id.into()));
        let container = &mut self.list;
        let op = op.clone();
        let id = YataImpl::id(&op);
        for _ in container.version_vector.len()..id.client_id + 1 {
            container.version_vector.push(0);
        }

        yata::integrate::<YataImpl>(&mut self.list, op.clone(), &mut ());
        self.list.version_vector[id.client_id] = id.clock + 1;
        self.len += 1;
        if !is_local {
            let (_, arr_index) = index(&self.list, id.into());
            self._range_insert(1, &op, arr_index, is_local);
        }
    }

    #[allow(unused)]
    fn check(&mut self) {
        assert_eq!(self.len, self.list.content.real_len());
        assert_eq!(self.len * 3 + 2, self.range.range_map.len());
        assert!(self
            .range_ops
            .iter()
            .all(|x| x.lamport() < self.next_lamport));
        let range_op_id_set: HashSet<OpID> = self.range_ops.iter().map(|x| x.id()).collect();
        for op in self.list.content.iter() {
            // no intersection
            assert!(!range_op_id_set.contains(&op.id.into()));
            assert!(self.visited.contains(&op.id.into()));
        }
        for id in range_op_id_set {
            assert!(self.visited.contains(&id));
        }

        let mut ann_set = BTreeSet::new();
        for span in self.range.get_annotations(..) {
            for ann in span.annotations.iter() {
                ann_set.insert(ann.clone());
            }
        }

        for ann in ann_set {
            let ann_range = self.range.get_annotation_range(ann.id).unwrap();
            let (mut start, start_deleted) = ann
                .range
                .start
                .id
                .map(|x| self.id_pos(x))
                .unwrap_or((0, true));
            if !start_deleted && ann.range.start.type_ == AnchorType::After {
                start += 1;
            }

            let (mut end, end_deleted) = ann
                .range
                .end
                .id
                .map(|x| self.id_pos(x))
                .unwrap_or((self.len, true));
            if !end_deleted && ann.range.end.type_ == AnchorType::After {
                end += 1;
            }

            let anchor_range = start..end;
            assert_eq!(ann_range, anchor_range);
        }
    }

    fn id_pos(&self, id: OpID) -> (usize, bool) {
        let mut index = 0;
        for item in self.list.content.iter() {
            let item_id: OpID = item.id.into();
            if item_id == id {
                return (index, item.deleted);
            }

            if !item.deleted {
                index += 1;
            }
        }

        panic!()
    }

    #[allow(unused)]
    fn check_eq(&mut self, other: &mut Self) {
        assert_eq!(self.len(), other.len());
        assert_eq!(self.list.content, other.list.content);
        assert_eq!(
            self.range.get_annotations(..),
            other.range.get_annotations(..)
        );
        assert_eq!(self.deleted, other.deleted);
    }

    #[allow(unused)]
    fn len(&self) -> usize {
        self.len
    }
}

fn get_insert_pos(pos: usize, container: &mut Container) -> usize {
    if pos == container.content.real_len() {
        container.content.len()
    } else {
        container.content.real_index(pos)
    }
}

pub fn make_spans(spans: &[(Vec<&str>, usize)]) -> Vec<SimpleSpan> {
    spans
        .iter()
        .map(|(annotations, len)| SimpleSpan {
            annotations: annotations.iter().map(|x| x.to_string()).collect(),
            len: *len,
        })
        .collect()
}

#[cfg(test)]
mod test;
