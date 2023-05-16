use std::io::prelude::*;
use std::ops::Deref;
use std::{hash::Hash, sync::Arc};

use append_only_bytes::AppendOnlyBytes;
use flate2::write::GzEncoder;
use flate2::{read::GzDecoder, Compression};
use fxhash::FxHashMap;
use generic_btree::rle::HasLength;
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, from_bytes, to_vec};

use crate::{
    Anchor, AnchorRange, AnchorType, Annotation, Behavior, ClientID, InternalString, OpID,
};

use super::op::{DeleteOp, Op, OpContent, TextInsertOp};
const COMPRESS_THRESHOLD: usize = 1024;

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct OpEncoding {
    #[columnar(strategy = "DeltaRle")]
    lamport: u32,
    #[columnar(strategy = "Rle")]
    type_: u8,
}

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct InsertEncoding {
    len: u32,
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
    start_client: u32,
    #[columnar(strategy = "DeltaRle")]
    start_counter: u32,
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
    /// index to ann_types_and_values
    type_: u32,
    /// index to ann_types_and_values
    value: u32,
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
    compressed_str: bool,
    clients: Vec<ClientID>,
    ann_types_and_values: Vec<InternalString>,
    op_len: Vec<u32>,
    start_counters: Vec<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpContentType {
    Insert = 0,
    Delete = 1,
    Ann = 2,
}

impl From<OpContentType> for u8 {
    fn from(value: OpContentType) -> Self {
        value as u8
    }
}

impl From<u8> for OpContentType {
    fn from(value: u8) -> Self {
        match value {
            0 => OpContentType::Insert,
            1 => OpContentType::Delete,
            2 => OpContentType::Ann,
            _ => unreachable!(),
        }
    }
}

type InnerUpdates = FxHashMap<ClientID, Vec<Op>>;

pub fn encode(exported: InnerUpdates) -> Vec<u8> {
    let data = to_doc_encoding(exported);
    to_vec(&data).unwrap()
}

pub fn decode(encoded: &[u8]) -> InnerUpdates {
    from_doc_encoding(from_bytes(encoded).unwrap())
}

fn to_doc_encoding(mut exported_map: InnerUpdates) -> DocEncoding {
    exported_map.retain(|_, v| !v.is_empty());
    let mut inserts = Vec::new();
    let mut deletes = Vec::new();
    let mut annotations = Vec::new();
    let mut client_mapping = VecMapping::new();
    for client in exported_map.keys() {
        client_mapping.get_or_insert(*client);
    }

    let mut ann_str_mapping = VecMapping::new();
    let mut op_len: Vec<u32> = Vec::new();
    let mut start_counters: Vec<u32> = Vec::new();
    let mut ops = Vec::with_capacity(exported_map.iter().map(|x| x.1.len()).sum());
    let mut str = Vec::new();

    for (_, op_arr) in exported_map.iter() {
        op_len.push(op_arr.len() as u32);
        start_counters.push(op_arr[0].id.counter);
        for op in op_arr {
            let type_ = match &op.content {
                crate::rich_text::op::OpContent::Text(text) => {
                    str.extend_from_slice(&text.text);
                    let zero = OpID::new(0, 0);
                    inserts.push(InsertEncoding {
                        len: text.text.len() as u32,
                        left_client: text
                            .left
                            .map(|x| client_mapping.get_or_insert(x.client) as u32)
                            .unwrap_or(u32::MAX),
                        left_counter: text.left.unwrap_or(zero).counter,
                        right_client: text
                            .right
                            .map(|x| client_mapping.get_or_insert(x.client) as u32)
                            .unwrap_or(u32::MAX),
                        right_counter: text.right.unwrap_or(zero).counter,
                    });
                    OpContentType::Insert
                }
                crate::rich_text::op::OpContent::Del(del) => {
                    deletes.push(DeleteEncoding {
                        start_client: client_mapping.get_or_insert(del.start.client) as u32,
                        start_counter: del.start.counter,
                        len: del.len,
                    });
                    OpContentType::Delete
                }
                crate::rich_text::op::OpContent::Ann(ann) => {
                    let start = ann.range.start.id;
                    let end = ann.range.end.id;
                    let type_ = ann_str_mapping.get_or_insert(ann.type_.clone());
                    let value = serde_json::to_string(&ann.value).unwrap();
                    let value = ann_str_mapping.get_or_insert(value.into());
                    annotations.push(AnnEncoding {
                        start,
                        is_start_before_anchor: ann.range.start.type_ == AnchorType::Before,
                        end,
                        is_end_before_anchor: ann.range.end.type_ == AnchorType::Before,
                        behavior: ann.behavior,
                        type_: type_ as u32,
                        value: value as u32,
                    });
                    OpContentType::Ann
                }
            };

            ops.push(OpEncoding {
                lamport: op.lamport,
                type_: type_.into(),
            });
        }
    }

    assert_eq!(op_len.len(), exported_map.len());
    assert_eq!(op_len.len(), start_counters.len());
    assert_eq!(op_len.iter().sum::<u32>() as usize, ops.len());
    debug_assert_eq!(
        str.len(),
        inserts.iter().map(|x| x.len).sum::<u32>() as usize
    );
    let mut compressed_str = false;
    if str.len() > COMPRESS_THRESHOLD {
        compressed_str = true;
        let mut e = GzEncoder::new(Vec::new(), Compression::default());
        e.write_all(&str).unwrap();
        str = e.finish().unwrap();
    }

    DocEncoding {
        ops,
        inserts,
        deletes,
        annotations,
        compressed_str,
        clients: client_mapping.vec,
        ann_types_and_values: ann_str_mapping.vec,
        op_len,
        start_counters,
        str,
    }
}

fn from_doc_encoding(exported: DocEncoding) -> InnerUpdates {
    let clients = &exported.clients;
    let mut str = AppendOnlyBytes::new();
    if exported.compressed_str {
        let mut d = GzDecoder::new(exported.str.deref());
        let mut ans = vec![];
        d.read_to_end(&mut ans).unwrap();
        str.push_slice(&ans);
    } else {
        str.push_slice(&exported.str);
    }
    let mut str_index = 0;
    let mut ans: InnerUpdates = Default::default();
    let mut insert_iter = exported.inserts.iter();
    let mut delete_iter = exported.deletes.iter();
    let mut ann_iter = exported.annotations.iter();
    let mut op_iter = exported.ops.iter();
    for ((client, op_len), counter) in exported
        .clients
        .iter()
        .zip(exported.op_len.iter())
        .zip(exported.start_counters.iter())
    {
        let mut counter = *counter;
        let mut arr = Vec::with_capacity((*op_len) as usize);
        for _ in 0..*op_len {
            let op = op_iter.next().unwrap();
            let id = OpID {
                client: *client,
                counter,
            };
            let content = match op.type_.into() {
                OpContentType::Insert => {
                    let insert = insert_iter.next().unwrap();
                    let left = if insert.left_client != u32::MAX {
                        Some(OpID {
                            client: clients[insert.left_client as usize],
                            counter: insert.left_counter,
                        })
                    } else {
                        None
                    };
                    let right = if insert.right_client != u32::MAX {
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
                            client: clients[delete.start_client as usize],
                            counter: delete.start_counter,
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
                        type_: exported.ann_types_and_values[ann.type_ as usize].clone(),
                        id,
                        range_lamport: (op.lamport, id),
                        value: serde_json::from_str(
                            &exported.ann_types_and_values[ann.value as usize],
                        )
                        .unwrap(),
                    }))
                }
            };

            let op = Op {
                id,
                lamport: op.lamport,
                content,
            };
            counter += op.rle_len() as u32;
            arr.push(op);
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
