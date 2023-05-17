use fxhash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_columnar::to_vec;

use crate::{ClientID, Counter};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct VersionVector {
    pub vv: FxHashMap<ClientID, Counter>,
}

#[derive(Serialize, Clone, Copy, Deserialize)]
struct Item {
    client: ClientID,
    counter: Counter,
}

impl VersionVector {
    pub fn encode(&self) -> Vec<u8> {
        let v: Vec<Item> = self
            .vv
            .iter()
            .map(|x| Item {
                client: *x.0,
                counter: *x.1,
            })
            .collect();
        to_vec(&v).unwrap()
    }

    pub fn decode(data: &[u8]) -> VersionVector {
        let v: Vec<Item> = serde_columnar::from_bytes(data).unwrap();
        let mut vv = VersionVector::default();
        for item in v {
            vv.vv.insert(item.client, item.counter);
        }
        vv
    }
}
