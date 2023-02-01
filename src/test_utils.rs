use std::collections::HashSet;

use super::range_map::test::{DumbRangeMap, Position};
use super::*;
use arbitrary::Arbitrary;
use crdt_list::crdt::ListCrdt;
use crdt_list::test::TestFramework;
use crdt_list::yata::{self, integrate, Yata};
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
                    if x.0.merge_method == RangeMergeRule::Delete {
                        None
                    } else {
                        Some(x.0.type_.clone())
                    }
                })
                .collect(),
        }
    }
}

pub struct Actor {
    list: Container,
    range: CrdtRange<DumbRangeMap>,
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
            *actor = *actor % actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len + 1)) as u8;
            *len = (*len).min(10);
            *len = (*len).max(1);
        }
        Action::Delete { actor, pos, len } => {
            *actor = *actor % actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len + 1)) as u8;
            *len = (*len).min(10);
            *len %= (actors[*actor as usize].len.max(*pos as usize + 1) - *pos as usize) as u8;
        }
        Action::Annotate {
            actor,
            pos,
            len,
            annotation,
        } => {
            *actor = *actor % actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len + 1)) as u8;
            *len = (*len).min(10);
            *len %= (actors[*actor as usize].len.max(*pos as usize + 1) - *pos as usize) as u8;
        }
        Action::Sync(a, b) => {
            *a = *a % actors.len() as u8;
            *b = *b % actors.len() as u8;
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
        }
        Action::Delete { actor, pos, len } => {
            if len == 0 {
                return;
            }

            actors[actor as usize].delete(pos as usize, len as usize);
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
                        .annotate(pos as usize..=pos as usize + len as usize, "link");
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
                        .un_annotate(pos as usize..=pos as usize + len as usize, "link");
                }
            }
        }
        Action::Sync(a, b) => {
            let (a, b) = arref::array_mut_ref!(actors, [a as usize, b as usize]);
            a.merge(b);
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
        apply_action(&mut actors, action);
    }

    for i in 0..actors.len() {
        for j in (i + 1)..actors.len() {
            let (a, b) = arref::array_mut_ref!(&mut actors, [i, j]);
            a.merge(b);
            b.merge(a);
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

        self._range_insert(pos, len, &op, arr_pos, true);
    }

    /// this should happen after the op is integrated to the list crdt
    fn _range_insert(
        &mut self,
        text_pos: usize,
        len: usize,
        first_op: &Op,
        arr_pos: usize,
        is_local: bool,
    ) {
        let mut range_ops =
            self.range
                .insert_text(text_pos, len, is_local, first_op.id.into(), |filter| {
                    let mut ans = vec![];
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

                    let left_op = if arr_pos != 0 {
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

                        for i in last_alive_arr_index..arr_pos {
                            if self.list.content[i].deleted {
                                let id: OpID = self.list.content[i].id.into();
                                if !filter.contains(&id) {
                                    ans.push(id);
                                }
                            }
                        }

                        self.list
                            .content
                            .get(last_alive_arr_index)
                            .map(|x| x.id.into())
                    } else {
                        None
                    };

                    ans.push(self.list.content[arr_pos].id.into());
                    for i in arr_pos + len..next_alive_arr_index {
                        assert!(self.list.content[i].deleted);
                        let id: OpID = self.list.content[i].id.into();
                        if !filter.contains(&id) {
                            ans.push(id);
                        }
                    }

                    (
                        left_op,
                        self.list
                            .content
                            .get(next_alive_arr_index)
                            .map(|x| x.id.into()),
                        ans,
                    )
                });
        if is_local {
            for op in range_ops.iter_mut() {
                op.set_id(self._use_next_id());
                op.set_lamport(self._use_next_lamport());
                self.visited.insert(op.id());
            }
        }
        self.range_ops.extend(range_ops);
    }

    pub fn delete(&mut self, pos: usize, len: usize) {
        self.len -= len;
        let op = YataImpl::new_del_op(&self.list, pos, len);
        YataImpl::integrate_delete_op(&mut self.list, op.clone());
        self.deleted.extend(op.into_iter());
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
        self.range_ops.push(self.range.annotate(
            Annotation {
                id,
                lamport,
                range: AnchorRange { start, end },
                merge_method,
                type_: type_.to_string(),
                meta: None,
            },
            range,
        ));
    }

    pub fn get_annotations(&self, range: impl RangeBounds<usize>) -> Vec<SimpleSpan> {
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
        assert_ne!(self.list.id, other.list.id);
        for op in other.list_ops.iter() {
            if !self.visited.contains(&op.id.into()) {
                self.integrate_insert_op(op, false);
                self.list_ops.push(op.clone());
                self.visited.insert(op.id.into());
            }
        }

        for op in other.range_ops.iter() {
            if !self.visited.contains(&op.id()) {
                self.range
                    .apply_remote_op(op.clone(), &|x| index(&self.list, x).0);
                self.range_ops.push(op.clone());
                self.visited.insert(op.id());
            }
        }

        let mut new_deleted: HashSet<ListOpId> = HashSet::new();
        for id in other.deleted.iter() {
            if !self.deleted.contains(id) {
                new_deleted.insert(*id);
                self.deleted.insert(*id);
                self.len -= 1;
            }
        }

        {
            let container = &mut self.list;
            let mut deleted_text = vec![];
            for (text_index, op) in container.content.iter_real_mut().enumerate() {
                if new_deleted.contains(&op.id) {
                    op.deleted = true;
                    deleted_text.push(text_index);
                }
            }

            for index in deleted_text.iter().rev() {
                self.range.delete_text(*index, 1);
            }
        };

        self.next_lamport = std::cmp::max(self.next_lamport, other.next_lamport);
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
            let (text_index, arr_index) = index(&self.list, id.into());
            self._range_insert(text_index.unwrap(), 1, &op, arr_index, is_local);
        }
    }

    fn check(&self) {
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
    }

    fn check_eq(&self, other: &Self) {
        assert_eq!(self.len(), other.len());
        assert_eq!(self.list.content, other.list.content);
        assert_eq!(
            self.range.get_annotations(..),
            other.range.get_annotations(..)
        );
        assert_eq!(self.deleted, other.deleted);
    }

    fn len(&self) -> usize {
        self.len
    }
}

fn get_insert_pos(pos: usize, container: &mut Container) -> usize {
    let insert_pos = if pos == container.content.real_len() {
        container.content.len()
    } else {
        container.content.real_index(pos)
    };

    insert_pos
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

#[test]
fn test_insert_text_after_bold() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    // **12345**67890
    actor.annotate(0..5, "bold");
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["bold"]), 5), ((vec![]), 5),]));
    // **12345xx**67890
    actor.insert(5, 2);
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["bold"]), 7), ((vec![]), 5),]));
    // **12345xx**6xx7890
    actor.insert(8, 2);
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["bold"]), 7), ((vec![]), 7),]));
}

#[test]
fn test_insert_after_link() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    actor.annotate(0..=4, "link");
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["link"]), 5), ((vec![]), 5),]));
    actor.insert(5, 2);
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["link"]), 5), ((vec![]), 7),]));
    actor.insert(4, 2);
    let spans = actor.get_annotations(..);
    assert_eq!(spans, make_spans(&[((vec!["link"]), 7), ((vec![]), 7),]));
}

#[test]
fn test_sync() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    actor.annotate(0..=4, "link");
    let mut actor_b = Actor::new(1);
    actor.insert(0, 1);
    actor.merge(&actor_b);
    actor_b.merge(&actor);
    actor.check();
    actor.check_eq(&actor_b);
}

#[test]
fn test_delete_annotation() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    actor.annotate(0..5, "bold");
    actor.un_annotate(0..3, "bold");
    let spans = actor.get_annotations(..);
    assert_eq!(
        spans,
        make_spans(&[((vec![]), 3), ((vec!["bold"]), 2), ((vec![]), 5),])
    );
    actor.un_annotate(3..6, "bold");
    assert_eq!(actor.get_annotations(..), make_spans(&[((vec![]), 10),]));
}

#[test]
fn test_delete_text_basic() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    actor.annotate(0..5, "bold");
    actor.delete(0, 2);
    assert_eq!(
        actor.get_annotations(..),
        make_spans(&[((vec!["bold"]), 3), ((vec![]), 5)])
    );
}

#[test]
fn test_delete_text_1() {
    let mut actor = Actor::new(0);
    actor.insert(0, 10);
    //**01234**56789
    actor.annotate(0..3, "bold");
    assert_eq!(
        actor.get_annotations(..),
        make_spans(&[((vec!["bold"]), 3), ((vec![]), 7)])
    );
    actor.insert(2, 2);
    assert_eq!(
        actor.get_annotations(..),
        make_spans(&[((vec!["bold"]), 5), ((vec![]), 7)])
    );
    //**012**6789
    actor.delete(3, 3);
    assert_eq!(
        actor.get_annotations(..),
        make_spans(&[((vec!["bold"]), 3), ((vec![]), 6)])
    );
}

#[test]
fn test_delete_text_then_insert() {
    let mut actor = Actor::new(0);
    let mut b = Actor::new(1);
    actor.insert(0, 10);
    // **ABCDE**FGHIJ
    actor.annotate(0..5, "bold");
    // **ABC**FGHIJ
    actor.delete(3, 2);
    // **ABCxx**FGHIJ
    actor.insert(4, 2);
    b.merge(&actor);
    assert_eq!(
        b.get_annotations(..),
        make_spans(&[(vec!["bold"], 3), (vec![], 7)])
    );
}

#[test]
fn test_patch_expand() {
    let mut a = Actor::new(0);
    let mut b = Actor::new(1);
    let mut c = Actor::new(2);
    a.insert(0, 5);
    b.merge(&a);
    a.delete(2, 2);
    b.annotate(0..=3, "link");
    b.insert(3, 2);
    c.merge(&b);
    c.insert(5, 1);
    a.merge(&b);
    b.merge(&a);
    assert_eq!(a.get_annotations(..), b.get_annotations(..));
    c.merge(&a);
    a.merge(&c);
    assert_eq!(a.get_annotations(..), c.get_annotations(..));
}

#[test]
fn madness() {
    let mut a = Actor::new(0);
    let mut b = Actor::new(1);
    a.insert(0, 5);
    a.annotate(0..2, "bold");
    a.annotate(0..=3, "link");
    a.delete(2, 2);
    a.insert(2, 1);
    assert_eq!(
        a.get_annotations(..),
        make_spans(&[(vec!["bold", "link"], 2), (vec!["bold"], 1), (vec![], 1)])
    );
    b.merge(&a);
    assert_eq!(a.get_annotations(..), b.get_annotations(..));
}

#[cfg(test)]
mod failed_tests {
    use super::*;
    use Action::*;
    use AnnotationType::*;

    #[test]
    fn fuzz() {
        fuzzing(
            2,
            vec![Annotate {
                actor: 0,
                pos: 0,
                len: 0,
                annotation: UnLink,
            }],
        )
    }
}
