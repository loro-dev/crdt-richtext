use fxhash::FxHashMap;

use crate::{ClientID, Counter, OpID};

#[derive(Default, Debug, Clone)]
pub struct VersionVector {
    pub(crate) vv: FxHashMap<ClientID, Counter>,
}

impl VersionVector {
    pub fn use_next(&mut self, client: ClientID) -> OpID {
        let counter = self.vv.entry(client).or_default();
        *counter += 1;
        OpID {
            client,
            counter: *counter - 1,
        }
    }
}
