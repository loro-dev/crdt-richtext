use std::{hash::Hash, sync::Arc};

use append_only_bytes::AppendOnlyBytes;
use fxhash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, from_bytes, to_vec};

use crate::{
    Anchor, AnchorRange, AnchorType, Annotation, Behavior, ClientID, InternalString, OpID,
};

use super::op::{DeleteOp, Op, OpContent, TextInsertOp};

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct OpEncoding {
    #[columnar(strategy = "Rle")]
    client_idx: u32,
    #[columnar(strategy = "DeltaRle")]
    counter: u32,
    #[columnar(strategy = "DeltaRle")]
    lamport: u32,
    type_: OpContentType,
}

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct InsertEncoding {
    len: u32,
    type_: AreInsertLeftRightNone, // 00 left: None, right: None
    #[columnar(strategy = "Rle")]
    left_client: u32,
    #[columnar(strategy = "DeltaRle")]
    left_counter: u32,
    #[columnar(strategy = "Rle")]
    right_client: u32,
    #[columnar(strategy = "DeltaRle")]
    right_counter: u32,
}

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DeleteEncoding {
    #[columnar(strategy = "Rle")]
    left_client: u32,
    #[columnar(strategy = "DeltaRle")]
    left_counter: u32,
    len: i32,
}

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct AnnEncoding {
    start: Option<OpID>,
    #[columnar(strategy = "Rle")]
    is_start_before_anchor: bool,
    end: Option<OpID>,
    #[columnar(strategy = "Rle")]
    is_end_before_anchor: bool,
    behavior: Behavior,
    type_: u32, // index to ann_types
}

#[columnar(ser, de)]
#[derive(Debug, Serialize, Deserialize)]
struct DocEncoding {
    #[columnar(type = "vec")]
    ops: Vec<OpEncoding>,
    #[columnar(type = "vec")]
    inserts: Vec<InsertEncoding>,
    #[columnar(type = "vec")]
    deletes: Vec<DeleteEncoding>,
    #[columnar(type = "vec")]
    annotations: Vec<AnnEncoding>,

    str: Vec<u8>,
    clients: Vec<ClientID>,
    ann_types: Vec<InternalString>,
    op_len: Vec<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpContentType {
    Insert = 0,
    Delete = 1,
    Ann = 2,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AreInsertLeftRightNone {
    BothNone = 0,
    LeftSomeRightNone = 1,
    LeftNoneRightSome = 2,
    BothSome = 3,
}

impl AreInsertLeftRightNone {
    fn from<T>((left, right): (Option<T>, Option<T>)) -> Self {
        match (left, right) {
            (None, None) => Self::BothNone,
            (Some(_), None) => Self::LeftSomeRightNone,
            (None, Some(_)) => Self::LeftNoneRightSome,
            (Some(_), Some(_)) => Self::BothSome,
        }
    }

    fn is_left_some(&self) -> bool {
        matches!(self, Self::LeftSomeRightNone | Self::BothSome)
    }

    fn is_right_some(&self) -> bool {
        matches!(self, Self::LeftNoneRightSome | Self::BothSome)
    }
}

type InnerUpdates = FxHashMap<ClientID, Vec<Op>>;

pub fn encode(exported: InnerUpdates) -> Vec<u8> {
    to_vec(&to_doc_encoding(exported)).unwrap()
}

pub fn decode(encoded: &[u8]) -> InnerUpdates {
    from_doc_encoding(from_bytes(encoded).unwrap())
}

fn to_doc_encoding(exported_map: InnerUpdates) -> DocEncoding {
    let mut inserts = Vec::new();
    let mut deletes = Vec::new();
    let mut annotations = Vec::new();
    let mut client_mapping = VecMapping::new();
    for client in exported_map.keys() {
        client_mapping.get_or_insert(*client);
    }

    let mut ann_types_mapping = VecMapping::new();
    let mut op_len: Vec<u32> = Vec::new();
    let mut ops = Vec::with_capacity(exported_map.iter().map(|x| x.1.len()).sum());
    let mut str = Vec::new();

    for (client, op_arr) in exported_map.iter() {
        op_len.push(op_arr.len() as u32);
        for op in op_arr {
            let type_ = match &op.content {
                crate::rich_text::op::OpContent::Text(text) => {
                    str.extend_from_slice(&text.text);
                    let zero = OpID::new(0, 0);
                    inserts.push(InsertEncoding {
                        len: text.text.len() as u32,
                        type_: AreInsertLeftRightNone::from((text.left, text.right)),
                        left_client: text
                            .left
                            .map(|x| client_mapping.get_or_insert(x.client) as u32)
                            .unwrap_or(0),
                        left_counter: text.left.unwrap_or(zero).counter,
                        right_client: text
                            .right
                            .map(|x| client_mapping.get_or_insert(x.client) as u32)
                            .unwrap_or(0),
                        right_counter: text.right.unwrap_or(zero).counter,
                    });
                    OpContentType::Insert
                }
                crate::rich_text::op::OpContent::Del(del) => {
                    deletes.push(DeleteEncoding {
                        left_client: client_mapping.get_or_insert(del.start.client) as u32,
                        left_counter: del.start.counter,
                        len: del.len,
                    });
                    OpContentType::Delete
                }
                crate::rich_text::op::OpContent::Ann(ann) => {
                    let start = ann.range.start.id;
                    let end = ann.range.end.id;
                    let type_ = ann_types_mapping.get_or_insert(ann.type_.clone());
                    annotations.push(AnnEncoding {
                        start,
                        is_start_before_anchor: ann.range.start.type_ == AnchorType::Before,
                        end,
                        is_end_before_anchor: ann.range.end.type_ == AnchorType::Before,
                        behavior: ann.behavior,
                        type_: type_ as u32,
                    });
                    OpContentType::Ann
                }
            };

            ops.push(OpEncoding {
                client_idx: client_mapping.get_or_insert(*client) as u32,
                counter: op.id.counter,
                lamport: op.lamport,
                type_,
            });
        }
    }

    assert_eq!(op_len.len(), exported_map.len());
    assert_eq!(op_len.iter().sum::<u32>() as usize, ops.len());
    debug_assert_eq!(
        str.len(),
        inserts.iter().map(|x| x.len).sum::<u32>() as usize
    );
    DocEncoding {
        ops,
        inserts,
        deletes,
        annotations,
        clients: client_mapping.vec,
        ann_types: ann_types_mapping.vec,
        op_len,
        str,
    }
}

fn from_doc_encoding(exported: DocEncoding) -> InnerUpdates {
    let clients = &exported.clients;
    let mut str = AppendOnlyBytes::new();
    str.push_slice(&exported.str);
    let mut str_index = 0;
    let mut ans: InnerUpdates = Default::default();
    let mut insert_iter = exported.inserts.iter();
    let mut delete_iter = exported.deletes.iter();
    let mut ann_iter = exported.annotations.iter();
    let mut op_iter = exported.ops.iter();
    for (client, op_len) in exported.clients.iter().zip(exported.op_len.iter()) {
        let mut arr = Vec::with_capacity((*op_len) as usize);
        for _ in 0..*op_len {
            let op = op_iter.next().unwrap();
            let id = OpID {
                client: clients[op.client_idx as usize],
                counter: op.counter,
            };
            let content = match op.type_ {
                OpContentType::Insert => {
                    let insert = insert_iter.next().unwrap();
                    let left = if insert.type_.is_left_some() {
                        Some(OpID {
                            client: clients[insert.left_client as usize],
                            counter: insert.left_counter,
                        })
                    } else {
                        None
                    };
                    let right = if insert.type_.is_right_some() {
                        Some(OpID {
                            client: clients[insert.right_client as usize],
                            counter: insert.right_counter,
                        })
                    } else {
                        None
                    };
                    let end = str_index + insert.len as usize;
                    let text = str.slice(str_index..end);
                    str_index = end;
                    OpContent::Text(TextInsertOp { left, right, text })
                }
                OpContentType::Delete => {
                    let delete = delete_iter.next().unwrap();
                    OpContent::Del(DeleteOp {
                        start: OpID {
                            client: clients[delete.left_client as usize],
                            counter: delete.left_counter,
                        },
                        len: delete.len,
                    })
                }
                OpContentType::Ann => {
                    let ann = ann_iter.next().unwrap();
                    let range = AnchorRange {
                        start: Anchor {
                            id: ann.start,
                            type_: if ann.is_start_before_anchor {
                                AnchorType::Before
                            } else {
                                AnchorType::After
                            },
                        },
                        end: Anchor {
                            id: ann.end,
                            type_: if ann.is_end_before_anchor {
                                AnchorType::Before
                            } else {
                                AnchorType::After
                            },
                        },
                    };

                    OpContent::Ann(Arc::new(Annotation {
                        range,
                        behavior: ann.behavior,
                        type_: exported.ann_types[ann.type_ as usize].clone(),
                        id,
                        range_lamport: (op.lamport, id),
                        meta: None,
                    }))
                }
            };

            arr.push(Op {
                id,
                lamport: op.lamport,
                content,
            });
        }

        ans.insert(*client, arr);
    }

    ans
}

struct VecMapping<T> {
    vec: Vec<T>,
    map: FxHashMap<T, usize>,
}

impl<T: Eq + Hash + Clone> VecMapping<T> {
    fn new() -> Self {
        Self {
            vec: Vec::new(),
            map: FxHashMap::default(),
        }
    }

    fn get(&self, idx: usize) -> &T {
        &self.vec[idx]
    }

    fn get_or_insert(&mut self, val: T) -> usize {
        if let Some(idx) = self.map.get(&val) {
            *idx
        } else {
            let idx = self.vec.len();
            self.vec.push(val.clone());
            self.map.insert(val, idx);
            idx
        }
    }
}
