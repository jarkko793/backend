use core::fmt;

use serde::{Deserialize, Serialize};
use wg_2024::network::NodeId;

use super::message::{SenderID, SessionID};

#[derive(Hash, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketID(pub SessionID, pub SenderID, pub FragmentID);
#[derive(Hash, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketID2(pub SessionID, pub SenderID);

#[derive(Debug, Hash, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentID(pub u64);

impl fmt::Display for PacketID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}:{}", self.0, self.1, self.2)
    }
}

impl fmt::Display for FragmentID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::database) struct DatabasePacket {
    pub packet_id: String,
    pub routing_header_hop_index: usize,
    pub routing_header_hops: Vec<NodeId>,
    pub session_id: String,
    pub sender_id: u8,
    pub fragment_index: String,
    pub total_n_fragments: String,
    pub length: u8,
    pub data: Vec<u8>,
    pub sent_to_sc: bool,
    pub ack_received: bool,
}
