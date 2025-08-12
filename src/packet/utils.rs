#![allow(dead_code)]
use anyhow::{Result, anyhow};
use assembler::Assembler;
use assembler::naive_assembler::NaiveAssembler;
use messages::{Message, MessageUtilities};
use wg_2024::{
    network::SourceRoutingHeader,
    packet::{FloodRequest, NodeType, Packet, PacketType},
};

/// Converts a `Message` into a vector of `Packet` fragments suitable for sending.
///
/// The message is first stringified, then disassembled into fragments by the
/// `NaiveAssembler`. Each fragment is wrapped in a `Packet` with the given
/// `routing_header` and message `session_id`.
///
/// # Arguments
///
/// * `message` - The message to convert into packets.
/// * `routing_header` - The source routing header to attach to each packet.
///
/// # Returns
///
/// A vector of packets, each containing a fragment of the original message.
pub fn message_to_packets(message: &Message, routing_header: &SourceRoutingHeader) -> Vec<Packet> {
    let message_as_string = message.stringify();
    let fragments = NaiveAssembler::disassemble(message_as_string.as_bytes());

    fragments
        .iter()
        .map(|fragment| {
            Packet::new_fragment(routing_header.clone(), message.session_id, fragment.clone())
        })
        .collect()
}

/// Reassembles a `Message` from a slice of `Packet`s containing message fragments.
///
/// Extracts all message fragments from the packets, reassembles them using
/// `NaiveAssembler`, and parses the resulting UTF-8 string into a `Message`.
///
/// # Arguments
///
/// * `packets` - Slice of packets potentially containing message fragments.
///
/// # Returns
///
/// Returns `Ok(Message)` if reassembly and parsing succeed, otherwise returns an error.
pub fn packets_to_message(packets: &[Packet]) -> Result<Message> {
    let fragments: Vec<_> = packets
        .iter()
        .filter_map(|packet| {
            if let PacketType::MsgFragment(fragment) = &packet.pack_type {
                Some(fragment.clone())
            } else {
                None
            }
        })
        .collect();

    let message = NaiveAssembler::reassemble(&fragments);
    let message = String::from_utf8(message)?;
    let message =
        <messages::Message as MessageUtilities>::from_string(message).map_err(|e| anyhow!(e))?;
    Ok(message)
}

/// Constructs a new `Packet` containing a `FloodRequest` with the given session ID and initiator ID.
///
/// The packet's routing header is an empty route. The flood request contains
/// the initiator's ID and a path trace starting with the initiator as a client node.
///
/// # Arguments
///
/// * `session_id` - The session ID to assign to the packet.
/// * `initiator_id` - The ID of the initiator node.
///
/// # Returns
///
/// A `Packet` initialized as a flood request.
pub fn get_new_flood_request_packet(session_id: u64, initiator_id: u8) -> Packet {
    let flood_request = FloodRequest {
        flood_id: session_id,
        path_trace: vec![(initiator_id, NodeType::Client)],
        initiator_id,
    };
    Packet {
        routing_header: SourceRoutingHeader::empty_route(),
        session_id,
        pack_type: PacketType::FloodRequest(flood_request),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use messages::{Message, MessageType, RequestType};
    use wg_2024::network::SourceRoutingHeader;
    use wg_2024::packet::{FloodRequest, NodeType, PacketType};

    // For test, assume NodeId is u8 (or define a mock NodeId)
    type NodeId = u8;

    fn make_test_message() -> Message {
        Message {
            source: 1 as NodeId,
            destination: 2 as NodeId,
            session_id: 42,
            content: MessageType::Request(RequestType::DiscoveryRequest(())),
        }
    }

    #[test]
    fn test_message_to_packets_and_back() {
        let message = make_test_message();
        let routing_header = SourceRoutingHeader::empty_route();

        let packets = message_to_packets(&message, &routing_header);
        assert!(!packets.is_empty());

        for packet in &packets {
            match &packet.pack_type {
                PacketType::MsgFragment(_) => (),
                _ => panic!("Expected MsgFragment packet type"),
            }
        }

        let reconstructed = packets_to_message(&packets).expect("Failed to reconstruct message");
        assert_eq!(message, reconstructed);
    }
    #[test]
    fn test_get_new_flood_request_packet() {
        let session_id = 123;
        let initiator_id = 45;
        let packet = get_new_flood_request_packet(session_id, initiator_id);

        assert_eq!(packet.session_id, session_id);
        match &packet.pack_type {
            PacketType::FloodRequest(flood) => {
                assert_eq!(flood.flood_id, session_id);
                assert_eq!(flood.initiator_id, initiator_id);
                assert_eq!(flood.path_trace.len(), 1);
                assert_eq!(flood.path_trace[0].0, initiator_id);
                assert_eq!(flood.path_trace[0].1, NodeType::Client);
            }
            _ => panic!("Expected FloodRequest packet type"),
        }
    }
}
