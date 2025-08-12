// TODO REMOVE CLIPPY ALLOWS
#![allow(dead_code)]
pub mod message;
pub mod packet;

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use message::{MessageID, SenderID, SessionID};
use messages::Message;
use packet::{FragmentID, PacketID, PacketID2};

pub(crate) use wg_2024::packet::{Packet, PacketType};

pub struct Database {
    messages: HashMap<MessageID, Message>,
    packets: HashMap<PacketID2, PacketStore>,
    packets_sent_to_sc: HashSet<PacketID>,
    messages_sent_to_sc: HashSet<MessageID>,
    messages_read: HashSet<MessageID>,
    packets_received_ack: HashSet<PacketID>,
}

impl Database {
    pub fn new() -> Self {
        Database {
            messages: HashMap::new(),
            packets: HashMap::new(),
            packets_sent_to_sc: HashSet::new(),
            messages_sent_to_sc: HashSet::new(),
            messages_read: HashSet::new(),
            packets_received_ack: HashSet::new(),
        }
    }
}

struct PacketStore {
    packets: HashMap<u64, Packet>,
    all_fragments_received: bool,
    total_amount_of_frags: u64,
    received_amount_of_frags: u64,
}

impl PacketStore {
    fn new(total_amount_of_frags: u64) -> Self {
        PacketStore {
            packets: HashMap::new(),
            all_fragments_received: false,
            total_amount_of_frags,
            received_amount_of_frags: 0,
        }
    }
}

impl Database {
    pub fn save_message(&mut self, message: &Message) {
        let message_id = MessageID(SessionID(message.session_id), SenderID(message.source));
        self.messages.insert(message_id, message.clone());
    }

    pub fn save_packet(&mut self, packet: Packet) -> Result<()> {
        let PacketType::MsgFragment(fragment) = &packet.pack_type else {
            return Err(anyhow!("Packet is not Fragment!"));
        };

        let session_id = SessionID(packet.session_id);
        let sender_id = SenderID(packet.routing_header.hops[0]);
        let fragment_id = fragment.fragment_index;
        let packet_id = PacketID2(session_id, sender_id);
        let amount_of_frags = fragment.total_n_fragments;

        let packet_store = self
            .packets
            .entry(packet_id)
            .or_insert(PacketStore::new(amount_of_frags));

        packet_store.packets.insert(fragment_id, packet);
        packet_store.received_amount_of_frags += 1;
        if packet_store.received_amount_of_frags == packet_store.total_amount_of_frags {
            packet_store.all_fragments_received = true;
        }
        Ok(())
    }

    pub fn get_message(&self, message_id: MessageID) -> Option<Message> {
        self.messages.get(&message_id).cloned()
    }

    fn is_message_sent_to_sc(&self, message_id: MessageID) -> bool {
        self.messages_sent_to_sc.contains(&message_id)
    }

    fn is_message_read(&self, message_id: MessageID) -> bool {
        self.messages_read.contains(&message_id)
    }
    fn is_packet_ack_received(&self, packet_id: PacketID) -> bool {
        self.packets_received_ack.contains(&packet_id)
    }

    fn is_packet_sent_to_sc(&self, packet_id: PacketID) -> bool {
        self.packets_sent_to_sc.contains(&packet_id)
    }

    pub fn get_packet(&self, packet_id: PacketID) -> Option<Packet> {
        let session_id = PacketID2(packet_id.0, packet_id.1);
        if let Some(packet_store) = self.packets.get(&session_id) {
            packet_store.packets.get(&packet_id.2.0).cloned()
        } else {
            None
        }
    }

    fn update_message_to_read(&mut self, message_id: MessageID) -> Result<()> {
        if self.messages.contains_key(&message_id) {
            self.messages_read.insert(message_id);
            Ok(())
        } else {
            Err(anyhow!(
                "Tried to update message read status but there is no such message!"
            ))
        }
    }

    fn update_message_sent_to_simulation_controller(
        &mut self,
        message_id: MessageID,
    ) -> Result<()> {
        if self.messages.contains_key(&message_id) {
            self.messages_sent_to_sc.insert(message_id);
            Ok(())
        } else {
            Err(anyhow!(
                "Tried to update message sent to SC status but there is no such message!"
            ))
        }
    }

    fn update_packet_sent_to_simulation_controller(&mut self, packet_id: PacketID) -> Result<()> {
        let session_id = PacketID2(packet_id.0, packet_id.1);
        if self.packets.contains_key(&session_id) {
            let packet_id = PacketID(packet_id.0, packet_id.1, packet_id.2);
            self.packets_sent_to_sc.insert(packet_id);
            Ok(())
        } else {
            Err(anyhow!(
                "Tried to update message sent to SC status but there is no such packet!"
            ))
        }
    }

    pub fn update_packet_ack_received(&mut self, packet_id: PacketID) -> Result<()> {
        let session_id = packet_id.0;
        let sender_id = packet_id.1;
        let session_id = &PacketID2(session_id, sender_id);

        if let Some(packet_store) = self.packets.get(session_id) {
            if packet_store.packets.contains_key(&packet_id.2.0) {
                self.packets_received_ack.insert(packet_id);
                Ok(())
            } else {
                Err(anyhow!(
                    "Tried to update packet's ACK status to received but there is no such packet!"
                ))
            }
        } else {
            Err(anyhow!(
                "Tried to update packet's ACK status to received but there is no packet stored with such session ID and sender ID!"
            ))
        }
    }

    pub fn get_undread_message_ids_from_server(&mut self, node_id: u8) -> Option<Vec<MessageID>> {
        let mut unread_messages: Vec<MessageID> = vec![];
        let read_messages = self.messages_read.clone();
        let read_messages: Vec<&MessageID> = read_messages.iter().collect();
        let all_messages: Vec<&MessageID> = self.messages.keys().collect();

        for message in all_messages {
            if !read_messages.contains(&message) && message.1 != SenderID(node_id) {
                unread_messages.push(*message);
                self.messages_read.insert(*message);
            }
        }

        if unread_messages.is_empty() {
            None
        } else {
            Some(unread_messages)
        }
    }

    pub fn get_amount_of_fragments_received(&self, session_id: u64, sender_id: u8) -> Option<u64> {
        let session_id = PacketID2(SessionID(session_id), SenderID(sender_id));

        self.packets
            .get(&session_id)
            .map(|packet_store| packet_store.received_amount_of_frags)
    }

    pub fn get_packets_for_session(&self, session_id: u64, sender_id: u8) -> Option<Vec<Packet>> {
        let session_id = PacketID2(SessionID(session_id), SenderID(sender_id));

        if let Some(packet_store) = self.packets.get(&session_id) {
            let mut packets = vec![];
            for p in packet_store.packets.values() {
                packets.push(p.clone());
            }
            Some(packets)
        } else {
            None
        }
    }

    //   all_packets_successfully_sent(sessionid) -> bool (sent to sim-controller after the fact. this can be checked when ack is received)
    pub fn all_packets_successfully_sent(&self, session: u64, sender_id: u8) -> Option<bool> {
        let session_id = PacketID2(SessionID(session), SenderID(sender_id));
        let mut all_received = true;

        if let Some(packet_store) = self.packets.get(&session_id) {
            for packet in packet_store.packets.keys() {
                let packet_id =
                    PacketID(SessionID(session), SenderID(sender_id), FragmentID(*packet));
                if !self.packets_received_ack.contains(&packet_id) {
                    all_received = false;
                }
            }
            Some(all_received)
        } else {
            None
        }
    }
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    // #[cfg(test)]
    use messages::{Message, MessageType, RequestType, TextRequest};
    #[allow(unused_imports)]
    use pretty_assertions::{assert_eq, assert_ne};
    use rand::{Rng, rng};
    use wg_2024::{
        network::SourceRoutingHeader,
        packet::{Ack, Fragment, Packet, PacketType},
    };

    use crate::database::{
        Database, MessageID, SenderID, SessionID,
        packet::{FragmentID, PacketID},
    };

    fn get_msg_with_random_session_id() -> Message {
        let session_id = rng().random_range(1..u64::MAX);
        Message {
            source: 1,
            destination: 2,
            session_id,
            content: MessageType::Request(RequestType::TextRequest(TextRequest::Text(
                "Hello".to_string(),
            ))),
        }
    }

    fn get_fragment_packet_with_random_session_id() -> Packet {
        let session_id = rng().random_range(1..u64::MAX);

        let hops = [3, 5, 6, 7, 4];

        let header = SourceRoutingHeader {
            hop_index: 0,
            hops: hops.to_vec(),
        };

        let fragment = Fragment {
            fragment_index: 0,
            total_n_fragments: 1,
            length: 13,
            data: [5; 128],
        };

        Packet {
            routing_header: header,
            session_id,
            pack_type: wg_2024::packet::PacketType::MsgFragment(fragment),
        }
    }

    #[allow(dead_code)]
    fn get_two_fragment_packets_with_random_session_id() -> Vec<Packet> {
        let session_id = rng().random_range(1..u64::MAX);
        let hops = [3, 5, 6, 7, 4];
        let mut packets = vec![];

        for n in 0..=1 {
            let header = SourceRoutingHeader {
                hop_index: 0,
                hops: hops.to_vec(),
            };
            let fragment = Fragment {
                fragment_index: n,
                total_n_fragments: 2,
                length: 58,
                data: [5; 128],
            };
            let packet = Packet {
                routing_header: header,
                session_id,
                pack_type: wg_2024::packet::PacketType::MsgFragment(fragment),
            };
            packets.push(packet);
        }
        packets
    }

    #[test]
    fn test_save_message() {
        // message to be saved
        let mut db = Database::new();
        let original_message = get_msg_with_random_session_id();
        db.save_message(&original_message);
        let message = db.get_message(MessageID(
            SessionID(original_message.session_id),
            SenderID(original_message.source),
        ));
        assert_eq!(original_message, message.unwrap());
    }

    #[test]
    fn test_get_message() {
        let mut db = Database::new();
        let message = get_msg_with_random_session_id();
        db.save_message(&message);
        let session_id = message.session_id;
        let sender_id = message.source;
        let queried_message = db
            .get_message(MessageID(SessionID(session_id), SenderID(sender_id)))
            .unwrap();
        assert_eq!(message, queried_message);
    }

    #[test]
    fn test_set_message_to_read() {
        let mut db = Database::new();

        let test_message = get_msg_with_random_session_id();
        let message_id = MessageID(
            SessionID(test_message.session_id),
            SenderID(test_message.source),
        );

        db.save_message(&test_message);

        let read_status = db.is_message_read(message_id);

        assert!(!read_status);

        db.update_message_to_read(message_id).unwrap();

        let read_status = db.is_message_read(MessageID(
            SessionID(test_message.session_id),
            SenderID(test_message.source),
        ));

        assert!(read_status);
    }

    #[test]
    fn test_set_message_to_sent_to_sc() {
        let mut db = Database::new();

        let test_message = get_msg_with_random_session_id();
        let message_id = MessageID(
            SessionID(test_message.session_id),
            SenderID(test_message.source),
        );

        db.save_message(&test_message);

        let sent_status = db.is_message_sent_to_sc(message_id);

        assert!(!sent_status);

        db.update_message_sent_to_simulation_controller(message_id)
            .unwrap();

        let sent_status = db.is_message_sent_to_sc(MessageID(
            SessionID(test_message.session_id),
            SenderID(test_message.source),
        ));

        assert!(sent_status);
    }

    #[test]
    fn test_save_packet() {
        let mut db = Database::new();

        // packet to be saved
        let original_packet = get_fragment_packet_with_random_session_id();
        let session_id = SessionID(original_packet.session_id);
        let sender_id = SenderID(3);
        let fragment_id = FragmentID(0);
        let packet_id = PacketID(session_id, sender_id, fragment_id);
        db.save_packet(original_packet.clone()).unwrap();
        let packet = db.get_packet(packet_id).unwrap();
        assert_eq!(original_packet, packet);
    }

    #[should_panic(expected = "Packet is not Fragment!")]
    #[test]
    fn test_save_wrong_type_packet() {
        let mut db = Database::new();

        let mut original_packet = get_fragment_packet_with_random_session_id();
        original_packet.pack_type = PacketType::Ack(Ack { fragment_index: 1 });
        db.save_packet(original_packet.clone()).unwrap();
    }

    #[test]
    fn test_set_packet_to_sent_to_sc() {
        let mut db = Database::new();

        let packet = get_fragment_packet_with_random_session_id();
        let session_id = SessionID(packet.session_id);
        let sender_id = SenderID(packet.routing_header.hops[0]);
        let fragment_id = FragmentID(0);
        let packet_id = PacketID(session_id, sender_id, fragment_id);

        db.save_packet(packet.clone()).unwrap();

        let sent_status = db.is_packet_sent_to_sc(packet_id);

        assert!(!sent_status);

        db.update_packet_sent_to_simulation_controller(packet_id)
            .unwrap();

        let sent_status = db.is_packet_sent_to_sc(packet_id);

        assert!(sent_status);
    }

    #[test]
    fn test_set_packet_to_ack_received() {
        let mut db = Database::new();

        let packet = get_fragment_packet_with_random_session_id();
        let session_id = SessionID(packet.session_id);
        let sender_id = SenderID(packet.routing_header.hops[0]);
        let fragment_id = FragmentID(0);
        let packet_id = PacketID(session_id, sender_id, fragment_id);

        db.save_packet(packet.clone()).unwrap();

        let ack_status = db.is_packet_ack_received(packet_id);

        assert!(!ack_status);

        db.update_packet_ack_received(packet_id).unwrap();

        let ack_status = db.is_packet_ack_received(packet_id);

        assert!(ack_status);
    }

    #[test]
    fn test_getting_amount_of_frags_received() {
        let mut db = Database::new();

        let packets = get_two_fragment_packets_with_random_session_id();
        let session_id = packets[0].session_id;
        let sender_id = packets[0].routing_header.hops[0];

        db.save_packet(packets[0].clone()).unwrap();
        let amount_of_frags_received = db
            .get_amount_of_fragments_received(session_id, sender_id)
            .unwrap();
        let expected_amount = 1;
        assert_eq!(amount_of_frags_received, expected_amount);

        db.save_packet(packets[1].clone()).unwrap();
        let amount_of_frags_received = db
            .get_amount_of_fragments_received(session_id, sender_id)
            .unwrap();
        let expected_amount = 2;
        assert_eq!(amount_of_frags_received, expected_amount);
    }

    #[test]
    fn test_getting_frags_for_a_session() {
        let mut db = Database::new();

        let packets = get_two_fragment_packets_with_random_session_id();
        let session_id = packets[0].session_id;
        let sender_id = packets[0].routing_header.hops[0];

        db.save_packet(packets[0].clone()).unwrap();
        let received_packets = db.get_packets_for_session(session_id, sender_id).unwrap();
        assert_eq!(received_packets.len(), 1);
        assert_eq!(packets[0], received_packets[0]);

        db.save_packet(packets[1].clone()).unwrap();
        let received_packets = db.get_packets_for_session(session_id, sender_id).unwrap();
        assert_eq!(received_packets.len(), 2);
        assert!(received_packets.contains(&packets[0]));
        assert!(received_packets.contains(&packets[1]));
    }

    #[test]
    fn test_are_all_acks_received() {
        let mut db = Database::new();

        let packets = get_two_fragment_packets_with_random_session_id();
        let session_id = packets[0].session_id;
        let sender_id = packets[0].routing_header.hops[0];

        // add packets
        db.save_packet(packets[0].clone()).unwrap();
        db.save_packet(packets[1].clone()).unwrap();
        // add ack for the first packet
        let _ = db.update_packet_ack_received(PacketID(
            SessionID(session_id),
            SenderID(sender_id),
            FragmentID(0),
        ));

        // assert that not all acks have been received
        let succssfully_sent = db
            .all_packets_successfully_sent(session_id, sender_id)
            .unwrap();
        assert!(!succssfully_sent);

        // add ack for the second packet
        let _ = db.update_packet_ack_received(PacketID(
            SessionID(session_id),
            SenderID(sender_id),
            FragmentID(1),
        ));

        // assert all acks have been received
        let succssfully_sent = db
            .all_packets_successfully_sent(session_id, sender_id)
            .unwrap();
        assert!(succssfully_sent);
    }
}
