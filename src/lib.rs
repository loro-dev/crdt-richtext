//! This CRDT lib combines [Peritext](https://www.inkandswitch.com/peritext/) and
//! [Fugue](https://arxiv.org/abs/2305.00583)'s power, delivering impressive performance
//! specifically tailored for rich text.
//!

#![deny(unsafe_code)]

use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashMap},
    fmt::Debug,
    ops::{Bound, Range, RangeBounds},
    sync::Arc,
};

use rich_text::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use string_cache::DefaultAtom;

pub mod legacy;
pub mod rich_text;
pub use rich_text::{vv::VersionVector, RichText};
mod small_set;
#[cfg(feature = "test")]
mod test_utils;
pub(crate) type InternalString = DefaultAtom;
type Lamport = u32;
type ClientID = u64;
type Counter = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OpID {
    client: ClientID,
    counter: Counter,
}

impl OpID {
    pub fn inc(&self, inc: Counter) -> Self {
        Self {
            client: self.client,
            counter: self.counter + inc as Counter,
        }
    }

    pub fn inc_i32(&self, inc: i32) -> Self {
        if inc > 0 {
            Self {
                client: self.client,
                counter: self.counter + inc as Counter,
            }
        } else {
            let (mut counter, overflow) = self.counter.overflowing_sub((-inc) as Counter);
            if overflow {
                counter = Counter::MAX;
            }

            Self {
                client: self.client,
                counter,
            }
        }
    }
}

pub(crate) struct IdSpan {
    id: OpID,
    len: Counter,
}

impl IdSpan {
    pub fn new(id: OpID, len: usize) -> Self {
        Self {
            id,
            len: len as Counter,
        }
    }

    pub fn contains(&self, id: OpID) -> bool {
        self.id.client == id.client
            && self.id.counter <= id.counter
            && id.counter < self.id.counter + self.len
    }
}

#[derive(Debug, Clone)]
pub enum RangeOp {
    Patch(Patch),
    Annotate(Annotation),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnchorType {
    Before,
    After,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum Behavior {
    /// When calculating the final state, it will keep all the ranges even if they have the same type
    ///
    /// For example, we would like to keep both comments alive even if they have overlapped regions
    AllowMultiple = 2,
    /// When calculating the final state, it will merge the ranges that have overlapped regions and have the same type
    ///
    /// For example, [bold 2~5] can be merged with [bold 1~4] to produce [bold 1-5]
    Merge = 0,
    /// It will delete the overlapped range that has smaller lamport && has the same type.
    /// But it will keep the `AllowMultiple` type unchanged
    Delete = 1,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    pub id: OpID,
    /// lamport value of the current range (it may be updated by patch)
    pub range_lamport: (Lamport, OpID),
    pub range: AnchorRange,
    pub behavior: Behavior,
    /// "bold", "comment", "italic", etc.
    pub type_: InternalString,
    pub value: Value,
}

impl PartialOrd for Annotation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.id.partial_cmp(&other.id) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }

        self.range_lamport.partial_cmp(&other.range_lamport)
    }
}

impl Ord for Annotation {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.id.cmp(&other.id) {
            core::cmp::Ordering::Equal => {}
            ord => return ord,
        }

        self.range_lamport.cmp(&other.range_lamport)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expand {
    None,
    Before,
    After,
    Both,
}

impl TryFrom<&str> for Expand {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "none" => Ok(Self::None),
            "start" => Ok(Self::Before),
            "after" => Ok(Self::After),
            "both" => Ok(Self::Both),
            _ => Err(()),
        }
    }
}

impl TryFrom<Option<&str>> for Expand {
    type Error = ();

    fn try_from(value: Option<&str>) -> Result<Self, Self::Error> {
        if let Some(value) = value {
            match value {
                "none" => Ok(Self::None),
                "start" => Ok(Self::Before),
                "after" => Ok(Self::After),
                "both" => Ok(Self::Both),
                _ => Err(()),
            }
        } else {
            Ok(Self::After)
        }
    }
}

impl Expand {
    pub fn infer_insert_expand(type_: &str) -> Self {
        match type_ {
            "comment" => Self::None,
            "header" => Self::None,
            "indent" => Self::None,
            "list" => Self::None,
            "align" => Self::None,
            "direction" => Self::None,
            "code-block" => Self::None,
            "code" => Self::None,
            "link" => Self::None,
            "script" => Self::None,
            "formula" => Self::None,
            "image" => Self::None,
            "video" => Self::None,
            _ => Self::After,
        }
    }

    pub fn infer_delete_expand(type_: &str) -> Self {
        Self::infer_insert_expand(type_).toggle()
    }

    /// For a target format, the Expand type of insertion is different
    /// from the Expand type of deletion. This method will convert one
    // to another.
    pub fn toggle(self) -> Self {
        match self {
            Self::None => Self::Both,
            Self::Before => Self::Before,
            Self::After => Self::After,
            Self::Both => Self::None,
        }
    }

    pub fn start_type(self) -> AnchorType {
        match self {
            Self::None => AnchorType::Before,
            Self::Before => AnchorType::After,
            Self::After => AnchorType::Before,
            Self::Both => AnchorType::After,
        }
    }

    pub fn end_type(self) -> AnchorType {
        match self {
            Self::None => AnchorType::After,
            Self::Before => AnchorType::After,
            Self::After => AnchorType::Before,
            Self::Both => AnchorType::Before,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    pub expand: Expand,
    pub behavior: Behavior,
    /// "bold", "comment", "italic", etc.
    pub type_: InternalString,
    pub value: Value,
}

impl Style {
    pub(crate) fn new_from_expand(
        expand: Expand,
        type_: InternalString,
        value: Value,
        behavior: Behavior,
    ) -> Result<Self, Error> {
        Ok(Style {
            expand,
            behavior,
            type_,
            value,
        })
    }

    pub fn new_bold_like(type_: InternalString, value: Value) -> Self {
        Self {
            expand: Expand::After,
            behavior: Behavior::Merge,
            type_,
            value,
        }
    }

    pub fn new_erase_bold_like(type_: InternalString) -> Self {
        Self {
            expand: Expand::After,
            behavior: Behavior::Delete,
            type_,
            value: Value::Null,
        }
    }

    pub fn new_link_like(type_: InternalString, value: Value) -> Self {
        Self {
            expand: Expand::None,
            behavior: Behavior::Merge,
            type_,
            value,
        }
    }

    pub fn new_erase_link_like(type_: InternalString) -> Self {
        Self {
            expand: Expand::Both,
            behavior: Behavior::Delete,
            type_,
            value: Value::Null,
        }
    }

    pub fn new_comment_like(type_: InternalString, value: Value) -> Self {
        Self {
            expand: Expand::None,
            behavior: Behavior::AllowMultiple,
            type_,
            value,
        }
    }

    #[inline(always)]
    pub fn start_type(&self) -> AnchorType {
        self.expand.start_type()
    }

    #[inline(always)]
    pub fn end_type(&self) -> AnchorType {
        self.expand.end_type()
    }
}

#[derive(Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct AnchorRange {
    pub start: Anchor,
    pub end: Anchor,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Anchor {
    /// if id is None, it means the anchor is at the beginning or the end of the document
    pub id: Option<OpID>,
    pub type_: AnchorType,
}

impl RangeOp {
    fn id(&self) -> OpID {
        match self {
            RangeOp::Patch(x) => x.id,
            RangeOp::Annotate(x) => x.id,
        }
    }

    #[allow(unused)]
    fn set_id(&mut self, id: OpID) {
        match self {
            RangeOp::Patch(x) => x.id = id,
            RangeOp::Annotate(x) => x.id = id,
        }
    }

    #[allow(unused)]
    fn lamport(&self) -> Lamport {
        match self {
            RangeOp::Patch(x) => x.lamport,
            RangeOp::Annotate(x) => x.range_lamport.0,
        }
    }
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

impl OpID {
    pub fn new(client: u64, counter: Counter) -> Self {
        Self { client, counter }
    }
}
