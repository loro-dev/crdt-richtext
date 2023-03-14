use fxhash::FxHashMap;

use crate::{ClientID, Counter, OpID};

#[derive(Default, Debug, Clone)]
pub struct VersionVector {
    pub(crate) vv: FxHashMap<ClientID, Counter>,
}

impl VersionVector {}
