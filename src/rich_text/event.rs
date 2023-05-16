use serde::{Deserialize, Serialize};

use super::{delta::DeltaItem, rich_tree::query::IndexType};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Event {
    pub ops: Vec<DeltaItem>,
    pub is_local: bool,
    pub index_type: IndexType,
}
