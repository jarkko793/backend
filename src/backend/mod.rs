//! Module provides public back-end functionality for Advanced Programming 2024 client.
#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use messages::{Message, node_event::NodeEvent};
use serde::{Deserialize, Serialize};
use wg_2024::{
    controller::DroneCommand,
    network::NodeId,
    packet::{NodeType, Packet},
};

use crate::network::router::Router;

pub struct Service {
    router: Router,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    GetEdgeNodesFromFlood,
    InitializeFlood,
    GetUnreadMessagesFromServer,
    GetClientsFromServer(u8),
    SendMessage(Message),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListOfDiscoveredEdgeNodes(pub Vec<(NodeId, NodeType)>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadMessagesFromServer(pub Vec<Message>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientsFromServer(pub Vec<u8>);

impl Service {
    /// Function will start the main loop of the back-end.
    /// Main loop consist of listening to incoming and outgoing packets
    /// as well as simulation controller related interactions.
    /// # Errors
    /// Propagates `Error`s encountered with crossbeam-channels.
    pub fn run(&mut self) -> !{
        self.router.listen_channels();
    }

    /// Creates a new back-end instance which can be then used to
    /// provide services for the front-end.
    ///
    /// # Returns
    /// Function returns new (back-end) `Service`-struct if the passed argument
    /// are valid.
    ///
    /// # Errors
    ///  If arguments are invalid, `String`-error is returned. Argument validity
    /// refers to the amount of connected neighbors; a client must be connected
    /// to at least one drone and at most to two. It must not be connected to
    /// other node types.
    pub fn new(
        node_id: u8,
        sc_event_channel: Sender<NodeEvent>,
        sc_command_channel: Receiver<DroneCommand>,
        neighbor_packet_channels: HashMap<NodeId, Sender<Packet>>,
        incoming_packet_channel: Receiver<Packet>,
        api_command_recv_channel: Receiver<Command>,
        outbound_response_for_flood: Sender<ListOfDiscoveredEdgeNodes>,
        outbound_undread_messages: Sender<UnreadMessagesFromServer>,
    ) -> Result<Self, String> {
        Self::validate_options(&neighbor_packet_channels, node_id)?;

        let router = Router::new(
            node_id,
            incoming_packet_channel,
            sc_command_channel,
            neighbor_packet_channels,
            sc_event_channel,
            api_command_recv_channel,
            outbound_response_for_flood,
            outbound_undread_messages,
        );
        let service = Service { router };
        Ok(service)
    }

    fn validate_options(
        neighbors: &HashMap<NodeId, Sender<Packet>>,
        node_id: u8,
    ) -> Result<(), String> {
        // Ensure that the client ID is not as a recipient
        if neighbors.contains_key(&node_id) {
            return Err(String::from("Own ID is used as a recipient."));
        }

        // Ensure that the amount of connected drones is valid.
        if neighbors.len() > 2 || neighbors.is_empty() {
            return Err(format!(
                "There are {} drones connected when the there must be 1-2 connected drones.",
                neighbors.len(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::unbounded;

    use super::*;

    // Helper to create dummy Sender<Packet>
    fn dummy_sender() -> Sender<Packet> {
        let (tx, _rx) = unbounded::<Packet>();
        tx
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn test_validate_options_own_id_as_recipient() {
        let node_id: NodeId = 1;
        let mut neighbors = HashMap::new();
        neighbors.insert(node_id, dummy_sender());

        let result = Service::validate_options(&neighbors, node_id);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Own ID is used as a recipient.");
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn test_validate_options_too_many_neighbors() {
        let node_id: NodeId = 1;
        let mut neighbors = HashMap::new();
        neighbors.insert(2, dummy_sender());
        neighbors.insert(3, dummy_sender());
        neighbors.insert(4, dummy_sender()); // > 2 neighbors

        let result = Service::validate_options(&neighbors, node_id);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "There are 3 drones connected when the there must be 1-2 connected drones."
        );
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn test_validate_options_no_neighbors() {
        let node_id: NodeId = 1;
        let neighbors = HashMap::new(); // empty

        let result = Service::validate_options(&neighbors, node_id);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "There are 0 drones connected when the there must be 1-2 connected drones."
        );
    }

    #[test]
    fn test_validate_options_valid_neighbors() {
        let node_id: NodeId = 1;
        let mut neighbors = HashMap::new();
        neighbors.insert(2, dummy_sender());

        let result = Service::validate_options(&neighbors, node_id);
        assert!(result.is_ok());

        neighbors.insert(3, dummy_sender()); // now 2 neighbors

        let result = Service::validate_options(&neighbors, node_id);
        assert!(result.is_ok());
    }
}
