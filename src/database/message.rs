use core::fmt;

use serde::{Deserialize, Serialize};

#[derive(Hash, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageID(pub SessionID, pub SenderID);
#[derive(Hash, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionID(pub u64);
#[derive(Hash, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderID(pub u8);

impl fmt::Display for SessionID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl fmt::Display for SenderID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for MessageID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.0, self.1)
    }
}
