use std::{
    collections::HashMap,
    ops::{Bound, RangeBounds},
    sync::Arc,
};

use range_map::{RangeMap, Span};

mod range_map;
type Lamport = u32;
type ClientID = u64;
type Counter = u32;

#[derive(Debug, Clone)]
pub enum RangeOp {
    Patch(Patch),
    Annotate(Annotation),
}
impl RangeOp {
    fn id(&self) -> OpID {
        match self {
            RangeOp::Patch(x) => x.id,
            RangeOp::Annotate(x) => x.id,
        }
    }

    fn set_id(&mut self, id: OpID) {
        match self {
            RangeOp::Patch(x) => x.id = id,
            RangeOp::Annotate(x) => x.id = id,
        }
    }

    fn lamport(&self) -> Lamport {
        match self {
            RangeOp::Patch(x) => x.lamport,
            RangeOp::Annotate(x) => x.lamport,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Anchor {
    /// if id is None, it means the anchor is at the beginning or the end of the document
    pub id: Option<OpID>,
    pub type_: AnchorType,
}

impl Anchor {
    pub fn before(id: OpID) -> Self {
        Self {
            id: Some(id),
            type_: AnchorType::Before,
        }
    }

    pub fn after(id: OpID) -> Self {
        Self {
            id: Some(id),
            type_: AnchorType::After,
        }
    }

    pub fn before_none() -> Self {
        Self {
            id: None,
            type_: AnchorType::Before,
        }
    }

    pub fn after_none() -> Self {
        Self {
            id: None,
            type_: AnchorType::After,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct AnchorRange {
    pub start: Anchor,
    pub end: Anchor,
}

impl<T: RangeBounds<OpID>> From<T> for AnchorRange {
    fn from(range: T) -> Self {
        let start = match range.start_bound() {
            Bound::Included(x) => Anchor {
                id: Some(*x),
                type_: AnchorType::Before,
            },
            Bound::Excluded(x) => Anchor {
                id: Some(*x),
                type_: AnchorType::After,
            },
            Bound::Unbounded => Anchor {
                id: None,
                type_: AnchorType::After,
            },
        };
        let end = match range.end_bound() {
            Bound::Included(x) => Anchor {
                id: Some(*x),
                type_: AnchorType::After,
            },
            Bound::Excluded(x) => Anchor {
                id: Some(*x),
                type_: AnchorType::Before,
            },
            Bound::Unbounded => Anchor {
                id: None,
                type_: AnchorType::Before,
            },
        };
        Self { start, end }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnchorType {
    Before,
    After,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RangeMergeRule {
    /// When calculating the final state, it will keep all the ranges even if they have the same type
    ///
    /// For example, we would like to keep both comments alive even if they have overlapped regions
    Inclusive,
    /// When calculating the final state, it will merge the ranges that have overlapped regions and have the same type
    ///
    /// For example, [bold 2~5] can be merged with [bold 1~4] to produce [bold 1-5]
    Merge,
    /// It will delete the overlapped range that has smaller lamport && has the same type
    Delete,
}

/// If both `move_start_to` and `move_end_to` equal to None, the target range will be deleted
#[derive(Clone, Copy, Debug)]
pub struct Patch {
    pub id: OpID,
    pub target_range_id: OpID,
    pub move_start_to: Option<OpID>,
    pub move_end_to: Option<OpID>,
    pub lamport: Lamport,
}

#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub struct Annotation {
    pub id: OpID,
    pub lamport: Lamport,
    pub range: AnchorRange,
    pub merge_method: RangeMergeRule,
    // TODO: use internal string
    /// "bold", "comment", "italic", etc.
    pub type_: String,
    pub meta: Option<Vec<u8>>,
}

// TODO: make it generic?
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpID {
    client: ClientID,
    counter: Counter,
}

impl OpID {
    pub fn new(client: ClientID, counter: Counter) -> Self {
        Self { client, counter }
    }
}

#[derive(Debug)]
pub struct CrdtRange<R> {
    range_map: R,
}

impl<R: RangeMap> CrdtRange<R> {
    pub fn new() -> Self {
        let mut r = R::init();
        r.insert(0, 2);
        CrdtRange { range_map: r }
    }

    /// `get_ops_at_pos(anchors)` returns the list of ops at `pos`.
    ///
    /// - The first returned element is the alive element at the pos
    /// - The rest of the elements are tombstones at the position filtered by `anchors` || is `first_new_op_id`
    pub fn insert_text<F>(
        &mut self,
        pos: usize,
        len: usize,
        first_new_op_id: OpID,
        get_ops_at_pos: F,
    ) -> Vec<RangeOp>
    where
        F: FnOnce(&[OpID]) -> (Option<OpID>, Vec<OpID>),
    {
        let mut ans = vec![];
        self.range_map.insert(pos * 2, len * 2);
        ans
    }

    pub fn delete_text(&mut self, pos: usize, len: usize) {
        self.range_map.delete(pos * 2 + 1, len * 2);
    }

    pub fn annotate(&mut self, annotation: Annotation, range: impl RangeBounds<usize>) -> RangeOp {
        let start = match range.start_bound() {
            Bound::Included(x) => *x * 2 + 1,
            Bound::Excluded(_) => unreachable!("Excluded start bound is not supported"),
            Bound::Unbounded => unreachable!("Unbound start bound is not supported"),
        };
        assert!(annotation.range.start.type_ != AnchorType::After);
        assert!(annotation.range.start.id.is_some());
        let end = match range.end_bound() {
            Bound::Included(x) => *x * 2 + 2,
            Bound::Excluded(x) => *x * 2 + 1,
            Bound::Unbounded => self.range_map.len(),
        };
        self.range_map
            .annotate(start, end - start, annotation.clone());
        RangeOp::Annotate(annotation)
    }

    pub fn delete_annotation(&mut self, lamport: Lamport, op_id: OpID, target_id: OpID) -> RangeOp {
        self.range_map.delete_annotation(target_id);
        RangeOp::Patch(Patch {
            id: op_id,
            target_range_id: target_id,
            move_start_to: None,
            move_end_to: None,
            lamport,
        })
    }

    pub fn apply_remote_op<Index>(&mut self, op: RangeOp, index: &Index)
    where
        Index: Fn(OpID) -> usize,
    {
        match op {
            RangeOp::Patch(_) => todo!(),
            RangeOp::Annotate(a) => {
                let start = a.range.start.id.map(index).unwrap_or(0);
                let start = start * 2 + 1;
                let mut end = a.range.end.id.map(index).unwrap_or(self.range_map.len());
                match a.range.end.type_ {
                    AnchorType::Before => end = end * 2 + 1,
                    AnchorType::After => end = end * 2 + 2,
                }
                self.range_map.annotate(start, end - start, a)
            }
        }
    }

    pub fn get_annotations(&self, range: impl RangeBounds<usize>) -> Vec<Span> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => x * 2 + 1,
            std::ops::Bound::Excluded(_) => unreachable!(),
            std::ops::Bound::Unbounded => 1,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(x) => x * 2 + 3,
            std::ops::Bound::Excluded(x) => x * 2 + 1,
            std::ops::Bound::Unbounded => self.range_map.len() - 1,
        };
        let mut text_at_even_start = true;
        self.range_map
            .get_annotations(start, end - start)
            .into_iter()
            .filter_map(|mut x| {
                let mut annotations: HashMap<String, (Lamport, Vec<Arc<Annotation>>)> =
                    HashMap::new();
                for a in std::mem::take(&mut x.annotations) {
                    if let Some(x) = annotations.get_mut(&a.type_) {
                        if a.merge_method == RangeMergeRule::Inclusive {
                            x.1.push(a);
                        } else if a.lamport > x.0 {
                            *x = (a.lamport, vec![a]);
                        }
                    } else {
                        annotations.insert(a.type_.clone(), (a.lamport, vec![a]));
                    }
                }
                x.annotations = annotations.into_values().flat_map(|x| x.1).collect();
                let is_odd = x.len % 2 == 1;
                if text_at_even_start {
                    x.len = (x.len + 1) / 2;
                } else {
                    x.len /= 2;
                }
                if is_odd {
                    text_at_even_start = !text_at_even_start;
                }

                if x.len == 0 {
                    None
                } else {
                    Some(x)
                }
            })
            .collect()
    }
}

impl<R: RangeMap> Default for CrdtRange<R> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(feature = "test", test))]
mod test {
    use std::collections::HashSet;

    use super::range_map::dumb::{DumbRangeMap, Position};
    use super::*;
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
        range: CrdtRange<DumbRangeMap>,
        visited: HashSet<OpID>,
        list_ops: Vec<Op>,
        range_ops: Vec<RangeOp>,
        deleted: HashSet<ListOpId>,
        next_lamport: Lamport,
        len: usize,
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
    fn index(list: &Container, target_id: OpID) -> (usize, usize) {
        let mut text_index = 0;
        let mut arr_index = 0;
        let mut found = false;
        for op in list.content.iter() {
            let id: ListOpId = op.id;
            if OpID::from(id) == target_id {
                found = true;
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

        (text_index, arr_index)
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
            let (list_insert_pos, op) = self._get_list_insert_op(pos);

            self._range_insert(pos, len, &op, list_insert_pos);

            self.integrate_insert_op(&op, false);
            self.visited.insert(op.id.into());
            self.list_ops.push(op);
            for i in 1..len {
                let (_, op) = self._get_list_insert_op(pos + i);
                self.integrate_insert_op(&op, false);
                self.visited.insert(op.id.into());
                self.list_ops.push(op);
            }
        }

        fn _range_insert(&mut self, pos: usize, len: usize, op: &Op, insert_pos: usize) {
            let mut range_ops = self.range.insert_text(pos, len, op.id.into(), |filter| {
                let mut ans = vec![];
                let mut index = insert_pos - 1;
                while self
                    .list
                    .content
                    .get(index)
                    .map(|x| x.deleted)
                    .unwrap_or(false)
                {
                    if index == 0 {
                        break;
                    }

                    index -= 1;
                }

                for i in index..insert_pos {
                    if self.list.content[i].deleted {
                        let id: OpID = self.list.content[i].id.into();
                        if !filter.contains(&id) {
                            ans.push(id);
                        }
                    }
                }

                (self.list.content.get(index).map(|x| x.id.into()), ans)
            });
            for op in range_ops.iter_mut() {
                op.set_id(self._use_next_id());
                self.visited.insert(op.id());
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
            for op in other.list_ops.iter() {
                if !self.visited.contains(&op.id.into()) {
                    self.integrate_insert_op(op, true);
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

        fn integrate_insert_op(&mut self, op: &Op, update_range: bool) {
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
            if update_range {
                let (text_index, arr_index) = index(&self.list, id.into());
                self._range_insert(text_index, 1, &op, arr_index);
            }
        }

        fn check(&self) {
            assert_eq!(self.len, self.list.content.real_len());
            assert_eq!(self.len * 2 + 2, self.range.range_map.len());
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
        actor.annotate(0..5, "bold");
        let spans = actor.get_annotations(..);
        assert_eq!(spans, make_spans(&[((vec!["bold"]), 5), ((vec![]), 5),]));
        actor.insert(5, 2);
        let spans = actor.get_annotations(..);
        assert_eq!(spans, make_spans(&[((vec!["bold"]), 7), ((vec![]), 5),]));
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
}
