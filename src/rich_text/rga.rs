use crate::{Lamport, OpID};

pub struct ListOp {
    id: OpID,
    left: Option<OpID>,
    lamport: Lamport,
}
