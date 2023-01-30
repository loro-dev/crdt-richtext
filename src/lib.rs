use std::ops::RangeBounds;

use range_map::{RangeMap, Span};

mod range_map;
type Lamport = u32;
type ClientID = u64;
type Counter = u32;

pub enum RangeOp {
    Patch(Patch),
    Annotate(Annotation),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Anchor {
    /// if id is None, it means the anchor is at the beginning or the end of the document
    pub id: Option<OpID>,
    pub type_: AnchorType,
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
    pub start: Anchor,
    pub end: Anchor,
    pub merge_method: RangeMergeRule,
    // TODO: use internal string
    /// "bold", "comment", "italic", etc.
    pub type_: String,
    pub meta: Option<Vec<u8>>,
}

// TODO: make it generic?
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpID {
    client: ClientID,
    counter: Counter,
}

impl OpID {
    pub fn new(client: ClientID, counter: Counter) -> Self {
        Self { client, counter }
    }
}

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
        self.range_map.insert(pos * 2 + 1, len * 2);
        ans
    }

    pub fn delete_text(&mut self, pos: usize, len: usize) {
        self.range_map.delete(pos * 2 + 1, len * 2);
    }

    pub fn annotate(
        &mut self,
        annotation: Annotation,
        start: Option<usize>,
        end: Option<usize>,
    ) -> RangeOp {
        let start = start.map(|x| x * 2 + 1).unwrap_or(0);
        let end = end.map(|x| x * 2 + 1).unwrap_or(self.range_map.len());
        self.range_map
            .annotate(start, end - start, annotation.clone());
        RangeOp::Annotate(annotation)
    }

    pub fn delete_range(&mut self, lamport: Lamport, op_id: OpID, target_id: OpID) -> RangeOp {
        self.range_map.delete_annotation(target_id);
        RangeOp::Patch(Patch {
            id: op_id,
            target_range_id: target_id,
            move_start_to: None,
            move_end_to: None,
            lamport,
        })
    }

    pub fn apply_remote_op(&mut self, op: RangeOp) {
        match op {
            RangeOp::Patch(_) => todo!(),
            RangeOp::Annotate(_) => todo!(),
        }
    }

    pub fn get_annotations(&self, range: impl RangeBounds<usize>) -> Vec<Span> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => x * 2 + 1,
            std::ops::Bound::Excluded(x) => x * 2 + 2,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(x) => x * 2 + 2,
            std::ops::Bound::Excluded(x) => x * 2 + 1,
            std::ops::Bound::Unbounded => self.range_map.len(),
        };
        self.range_map
            .get_annotations(start, end - start)
            .into_iter()
            .filter_map(|mut x| {
                x.len /= 2;
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
    use super::range_map::dumb::{DumbRangeMap, Position};
    use super::*;
    use crdt_list::crdt::ListCrdt;
    use crdt_list::test::TestFramework;
    use crdt_list::yata::{integrate, Yata};
    use crdt_list::yata_dumb_impl::{Container, Op, OpId as ListOpId, YataImpl};

    pub struct Actor {
        list: Container,
        range: CrdtRange<DumbRangeMap>,
        list_ops: Vec<Op>,
        next_lamport: Lamport,
    }

    impl From<ListOpId> for OpID {
        fn from(value: ListOpId) -> Self {
            OpID {
                client: value.client_id as ClientID,
                counter: value.clock as Counter,
            }
        }
    }

    impl Actor {
        pub fn new() -> Self {
            Self {
                ..Default::default()
            }
        }

        pub fn insert(&mut self, pos: usize, len: usize) {
            let (insert_pos, op) = self._get_list_insert_op(pos);

            let content = &mut self.list.content;
            self.range.insert_text(pos, len, op.id.into(), |filter| {
                let mut ans = vec![];
                let mut index = insert_pos - 1;
                while content.get(index).map(|x| x.deleted).unwrap_or(false) {
                    if index == 0 {
                        break;
                    }

                    index -= 1;
                }

                for i in index..insert_pos {
                    if content[i].deleted {
                        let id: OpID = content[i].id.into();
                        if !filter.contains(&id) {
                            ans.push(id);
                        }
                    }
                }

                (content.get(index).map(|x| x.id.into()), ans)
            });
            YataImpl::integrate(&mut self.list, op.clone());
            self.list_ops.push(op);
            for i in 1..len {
                let (_, op) = self._get_list_insert_op(pos + i);
                YataImpl::integrate(&mut self.list, op.clone());
                self.list_ops.push(op);
            }
        }

        pub fn annotate(&mut self, pos: usize, len: usize, type_: String) {
            let id = self._use_next_id();
            let lamport = self._use_next_lamport();
            self.range.annotate(
                Annotation {
                    id,
                    lamport,
                    start: Anchor {
                        id: self._list_real_index(pos),
                        type_: AnchorType::Before,
                    },
                    end: Anchor {
                        id: self._list_real_index(pos + len),
                        type_: AnchorType::After,
                    },
                    merge_method: RangeMergeRule::Merge,
                    type_,
                    meta: None,
                },
                Some(pos),
                Some(pos + len),
            );
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

        fn _list_real_index(&self, pos: usize) -> Option<OpID> {
            let list: &Container = &self.list;
            let insert_pos = if pos == list.content.real_len() {
                list.content.len()
            } else {
                list.content.real_index(pos)
            };

            list.content.get(insert_pos - 1).map(|x| x.id.into())
        }

        fn _get_list_insert_op(&mut self, pos: usize) -> (usize, Op) {
            let container: &mut Container = &mut self.list;
            let insert_pos = if pos == container.content.real_len() {
                container.content.len()
            } else {
                container.content.real_index(pos)
            };
            let op = {
                let (left, right) = (
                    container.content.get(insert_pos - 1).map(|x| x.id),
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
    }

    impl Default for Actor {
        fn default() -> Self {
            Self::new()
        }
    }

    #[test]
    fn test() {}
}
