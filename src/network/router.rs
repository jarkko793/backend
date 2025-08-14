#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;

// TODO remove
use anyhow::{Context, Result, anyhow};
use crossbeam_channel::{Receiver, Sender, select};
use log::{error, info};
use messages::Message;
use messages::node_event::NodeEvent;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, FloodResponse, NackType, Packet, PacketType};
use wg_2024::{controller::DroneCommand, packet::NodeType};

use super::graph::{NetGraph, Vertice};
use crate::backend::{self, Command, ListOfDiscoveredEdgeNodes, UnreadMessagesFromServer};
use crate::database::Database;
use crate::database::message::{MessageID, SenderID, SessionID};
use crate::database::packet::{FragmentID, PacketID};
use crate::packet;

pub struct Router {
    graph: NetGraph,
    node_id: u8,
    session_id: u64,
    inbound_packet_channel: Receiver<Packet>,
    inbound_sc_command_channel: Receiver<DroneCommand>,
    outbound_packet_channels: HashMap<NodeId, Sender<Packet>>,
    outbound_sc_event_channel: Sender<NodeEvent>,
    database: Database,
    inbound_api_command: Receiver<Command>,
    outbound_response_for_flood: Sender<ListOfDiscoveredEdgeNodes>,
    outbound_undread_messages: Sender<UnreadMessagesFromServer>,
}

impl Router {
    pub fn get_new_session_id(&mut self) -> u64 {
        self.session_id += 1;
        self.session_id
    }

    pub fn new(
        node_id: u8,
        inbound_packet_channel: Receiver<Packet>,
        inbound_sc_command_channel: Receiver<DroneCommand>,
        outbound_packet_channels: HashMap<NodeId, Sender<Packet>>,
        outbound_sc_event_channel: Sender<NodeEvent>,
        inbound_api_command: Receiver<Command>,
        outbound_response_for_flood: Sender<ListOfDiscoveredEdgeNodes>,
        outbound_undread_messages: Sender<UnreadMessagesFromServer>,
    ) -> Self {
        let graph = NetGraph::new(node_id);
        let database = Database::new();

        Router {
            session_id: 0,
            graph,
            node_id,
            inbound_packet_channel,
            inbound_sc_command_channel,
            outbound_packet_channels,
            outbound_sc_event_channel,
            database,
            inbound_api_command,
            outbound_response_for_flood,
            outbound_undread_messages,
        }
    }

    pub fn listen_channels(&mut self) -> !{
        loop {
            select! {

                recv(self.inbound_packet_channel) -> packet => {
                    match packet{
                        Ok(packet) => {
                            let result = self.process(packet.clone());
                            match result {
                                Ok(()) => {},
                                Err(e) => {
                                    error!("Tried to process packet {packet:?} but failed with error: {e}");
                                },
                            }
                        },
                        Err(e) => error!("Failed to receive a packet from channel! Error: {e}"),

                    }
                },

                recv(self.inbound_sc_command_channel) -> command => {
                    match command {
                        Ok(command) => {
                            match self.process_sc_command(&command.clone()){
                                Ok(()) => {},
                                Err(e) => {
                                    error!("Tried to process command {command:?} but failed with error: {e}");
                                },
                            }
                        },
                        Err(e) => error!("Failed to receive a command from channel! Error: {e}"),
                    }
                },

                recv(self.inbound_api_command) -> command => {
                    match command{
                        Ok(command) => {
                            let result = self.process_api_command(command.clone());
                            match result {
                                Ok(()) => {},
                                Err(e) => {
                                    error!("Tried to process command {command:?} but failed with error: {e}");
                                },
                            }
                        },
                        Err(e) => error!("Failed to receive a command from channel! Error: {e}"),

                    }
                },

            }
        }
    }

    fn send_message(&mut self, message: &mut Message) -> Result<()> {
        message.session_id = self.get_new_session_id();
        let destination = message.destination;
        let hops = self.get_route_to_node(destination).context({
        format!(
            "Tried to fragment message to packets. Failed to find a route to destination: {destination}",
        )
        })?.with_context(||"")?;
        let routing_header = SourceRoutingHeader::new(hops, 1);

        let packets = packet::utils::message_to_packets(message, &routing_header);
        self.outbound_sc_event_channel
            .send(NodeEvent::StartingMessageTransmission(message.clone()))?;
        self.database.save_message(message);
        for packet in packets {
            self.database.save_packet(packet.clone())?;
            self.send_packet(packet)?;
        }
        Ok(())
    }

    fn get_edge_nodes(&self) -> Option<Vec<(NodeId, NodeType)>> {
        self.graph.get_edge_nodes()
    }

    fn process_sc_command(&mut self, command: &DroneCommand) -> Result<()> {
        match command {
            DroneCommand::RemoveSender(node_id) => {
                info!("Received SC command to remove {node_id} from neighbors.");
                self.outbound_packet_channels.remove(node_id);
                info!("{node_id} removed from neighbors.");
                self.flood_network()?;
            }
            DroneCommand::AddSender(node_id, channel) => {
                info!("Received SC command to add {node_id} to neighbors.");
                self.outbound_packet_channels
                    .insert(*node_id, channel.clone());
                self.flood_network()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn process_api_command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::GetEdgeNodesFromFlood => {
                let edge_nodes = self.get_edge_nodes();
                if let Some(edge_nodes) = edge_nodes {
                    self.outbound_response_for_flood
                        .send(backend::ListOfDiscoveredEdgeNodes(edge_nodes))?;
                }
            }
            Command::InitializeFlood => self.flood_network()?,
            Command::SendMessage(mut message) => {
                self.send_message(&mut message)?;
            }

            Command::GetUnreadMessagesFromServer => {
                if let Some(unread_message_ids) = self
                    .database
                    .get_undread_message_ids_from_server(self.node_id)
                {
                    let mut unread_messages = vec![];
                    for id in unread_message_ids {
                        if let Some(message) = self.database.get_message(id) {
                            unread_messages.push(message);
                        }
                    }
                    self.outbound_undread_messages
                        .send(backend::UnreadMessagesFromServer(unread_messages))?;
                }
            }
            // TODO this could be removed
            Command::GetClientsFromServer(_server_id) => {}
        }
        Ok(())
        //     Command::GetUnreadMessagesFromServer => {
        //     }
    }

    fn flood_network(&self) -> Result<()> {
        let packet = packet::utils::get_new_flood_request_packet(self.session_id, self.node_id);
        for neighbor in &self.outbound_packet_channels {
            neighbor.1.send(packet.clone()).with_context(|| {
                format!("Failed to send flood packet to neighbor {}.", neighbor.0)
            })?;
            self.outbound_sc_event_channel
                .send(NodeEvent::PacketSent(packet.clone()))
                .with_context(
                    || "Failed to send packet to SC after sending packet to a neighbor!",
                )?;
        }
        Ok(())
    }

    fn send_packet(&self, packet: Packet) -> Result<()> {
        let neighbor = packet
            .routing_header
            .hops
            .get(packet.routing_header.hop_index)
            .with_context(|| "")?;
        let neighbor_channel = self
            .outbound_packet_channels
            .get(neighbor)
            .with_context(|| {
                format!(
                    "Failed to send a packet. Client does not have a neighbor with ID {neighbor}!"
                )
            })?;
        neighbor_channel
            .send(packet.clone())
            .with_context(|| format!("Failed to send packet to neighbor {neighbor}."))?;
        self.outbound_sc_event_channel
            .send(NodeEvent::PacketSent(packet))
            .with_context(|| "Failed to send packet to SC after sending packet to a neighbor!")?;
        Ok(())
    }

    fn get_route_to_node(&self, destination_node: u8) -> Result<Option<Vec<u8>>> {
        let from = Vertice::new((self.node_id, NodeType::Client));
        let node_type = self.graph.get_node_type(destination_node)?;
        let to = Vertice::new((destination_node, node_type));
        Ok(self.graph.get_random_route(from, to))
    }

    fn add_route(&mut self, route: &[(u8, NodeType)]) -> Result<()> {
        self.graph.add_route(route, &self.outbound_sc_event_channel)
    }

    fn process(&mut self, packet: Packet) -> Result<()> {
        match packet.pack_type {
            PacketType::MsgFragment(_) => self.process_fragment(&packet)?,
            PacketType::Ack(_) => self.process_ack(&packet)?,
            PacketType::Nack(_) => self.process_nack(packet)?,
            PacketType::FloodRequest(mut flood_request) => {
                self.process_flood_request(&mut flood_request)?;
            }
            PacketType::FloodResponse(flood_response) => {
                self.process_flood_response(&flood_response)?;
            }
        }
        Ok(())
    }

    fn process_fragment(&mut self, packet: &Packet) -> Result<()> {
        let PacketType::MsgFragment(fragment) = &packet.pack_type else {
            return Err(anyhow!("Packet is not Fragment! Packet: {packet:?}"));
        };
        self.database.save_packet(packet.clone())?;
        let session_id = packet.session_id;
        let total_amount_of_frags = fragment.total_n_fragments;
        let sender_id = packet.routing_header.source().with_context(|| {
        format!(
            "Received fragment packet without sender in source routing header! Packet: {packet} ",
        )
    })?;
        let amount_of_frags_received = self.database.get_amount_of_fragments_received(
        session_id, sender_id,
    )
    .with_context(|| {
        format!(
            "Failed to query amount of fragments for session {session_id} from sender {sender_id}"
        )
    })?;
        //     fetch packets
        if amount_of_frags_received == total_amount_of_frags {
            let packets = self.database.get_packets_for_session(session_id, sender_id);
            let packets = packets.with_context(
                || "Received all fragments but failed to fetch them to build a message",
            )?;
            //     build message
            let message = packet::utils::packets_to_message(&packets)?;
            //     save message do db
            self.database.save_message(&message);
            self.outbound_sc_event_channel
                .send(NodeEvent::MessageReceived(message.clone()))
                .with_context(|| {
                    format!("Received a message but failed to send it to SC. Message: {message:?}",)
                })?;
        }
        Ok(())
    }

    fn process_ack(&mut self, packet: &Packet) -> Result<()> {
        let PacketType::Ack(ack) = &packet.pack_type else {
            return Err(anyhow!("Packet is not ACK! Packet: {packet:?}"));
        };

        let packet_id = PacketID(
            SessionID(packet.session_id),
            SenderID(self.node_id),
            FragmentID(ack.fragment_index),
        );
        self.database.update_packet_ack_received(packet_id)?;
        let message_fully_sent = self
            .database
            .all_packets_successfully_sent(packet_id.0.0, packet_id.1.0);
        let message_fully_sent = message_fully_sent.with_context(|| {
        format!("Received ACK {packet:?} but did not find such a session from DB while querying if all packets have been sent!")
    })?;
        if message_fully_sent {
            let message_id = MessageID(SessionID(packet.session_id), SenderID(self.node_id));
            let message = self.database.get_message(message_id);
            let message = message.with_context(|| format!("All packets have been ACKed for session {} but did not find message for such a session!", packet_id.0.0))?;

            self.outbound_sc_event_channel
            .send(NodeEvent::MessageSentSuccessfully(message))
            .with_context(|| format!("Received final ACK for session {}. Failed to send SC a copy of the sent message!", packet_id.0.0))?;
        }
        Ok(())
    }

    fn process_nack(&self, packet: Packet) -> Result<()> {
        let PacketType::Nack(nack) = packet.pack_type else {
            return Err(anyhow!("Packet is not NACK! Packet: {packet:?}"));
        };

        let packet_id = PacketID(
            SessionID(packet.session_id),
            SenderID(self.node_id),
            FragmentID(nack.fragment_index),
        );

        let Some(mut packet) = self.database.get_packet(packet_id) else {
            return Err(anyhow!("Failed to fetch packet from database!",));
        };

        match nack.nack_type {
            NackType::ErrorInRouting(_) => {
                // Route was incorrect. Initialize flood and resend packet
                self.flood_network()?;
                let destination = packet.routing_header.destination().with_context(
                    || "Tried to set a new route to a packet. The old routing header was empty!",
                )?;
                let new_route = self.get_route_to_node(destination).with_context(|| format!("Tried to set a new route to a packet. Did not find a route to the destination {destination}"))?;

                packet.routing_header.hops = new_route.with_context(|| "")?;

                self.send_packet(packet)?;
            }
            NackType::DestinationIsDrone => {
                // Server would have given wrong information since clients
                // do not interact in P2P manner. Therefore log error and
                // inform client.
                // TODO create a channel to give info to the front end.
                // Or just don't do anything?
            }
            NackType::Dropped => {
                // Packet was only dropped, therefore re-sending it should be enough.
                let destination = packet.routing_header.destination().with_context(
                    || "Tried to set a new route to a packet. The old routing header was empty!",
                )?;
                let new_route = self.get_route_to_node(destination).with_context(|| format!("Tried to set a new route to a packet. Did not find a route to the destination {destination}"))?;

                packet.routing_header.hops = new_route.with_context(|| "")?;

                self.send_packet(packet)?;
            }
            NackType::UnexpectedRecipient(_) => {
                // This is a drone error, a drone would have
                // sent a packet to a wrong neighbor.
                self.flood_network()?;
                let destination = packet.routing_header.destination().with_context(
                    || "Tried to set a new route to a packet. The old routing header was empty!",
                )?;
                let new_route = self.get_route_to_node(destination).with_context(|| format!("Tried to set a new route to a packet. Did not find a route to the destination {destination}"))?;

                packet.routing_header.hops = new_route.with_context(|| "")?;

                self.send_packet(packet)?;
            }
        }
        Ok(())
    }

    fn process_flood_request(&mut self, floodrequest: &mut FloodRequest) -> Result<()> {
        floodrequest
            .path_trace
            .push((self.node_id, NodeType::Client));
        let mut packet = floodrequest.generate_response(self.get_new_session_id());
        packet.routing_header.hop_index = 1;
        self.send_packet(packet).with_context(|| {
            format!("Failed to send flood response to a flood request {floodrequest}.",)
        })?;
        Ok(())
    }

    fn process_flood_response(&mut self, flood_response: &FloodResponse) -> Result<()> {
        self.add_route(&flood_response.path_trace)
    }
}
