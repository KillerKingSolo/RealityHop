//! A VLESS + REALITY node, as the feed and the configs describe it.

use serde::{Deserialize, Serialize};

/// The fields in engine.py's dict order: server, uuid, pbk, sid, sni, fp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub server: String,
    pub uuid: String,
    pub pbk: String,
    pub sid: String,
    pub sni: String,
    pub fp: String,
}
