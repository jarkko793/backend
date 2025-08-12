#![allow(dead_code)]

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use log::info;
use messages::node_event::{EventNetworkGraph, EventNetworkNode, NodeEvent};
use petgraph::{algo::simple_paths, visit::Visitable};
use rand::{Rng, rng};
use serde::{Deserialize, Serialize};
use wg_2024::{network::NodeId, packet::NodeType};

/// Represents a node in the network graph, storing its ID and type.
#[derive(Debug, Copy, PartialOrd, Ord, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vertice {
    node_id: u8,
    node_type: NodeTypeWrapper,
}

/// Internal wrapper around `NodeType` that can be used as a graph node key.
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Hash, PartialEq, Eq, Serialize, Deserialize)]
enum NodeTypeWrapper {
    Client,
    Drone,
    Server,
}

/// A directed network graph that stores vertices (`Vertice`) and their connections.
///
/// Internally uses `petgraph::graphmap::DiGraphMap` and ensures edges are bidirectional.
pub struct NetGraph {
    graph: petgraph::graphmap::DiGraphMap<Vertice, ()>,
    node_id: u8,
}

impl Vertice {
    /// Creates a new vertex from a `(NodeId, NodeType)` tuple.
    pub fn new(node: (NodeId, NodeType)) -> Self {
        let node_type = match node.1 {
            NodeType::Client => NodeTypeWrapper::Client,
            NodeType::Drone => NodeTypeWrapper::Drone,
            NodeType::Server => NodeTypeWrapper::Server,
        };
        Vertice {
            node_id: node.0,
            node_type,
        }
    }

    /// Returns the `NodeType` of this vertex.
    pub fn get_node_type(self) -> NodeType {
        match self.node_type {
            NodeTypeWrapper::Client => NodeType::Client,
            NodeTypeWrapper::Drone => NodeType::Drone,
            NodeTypeWrapper::Server => NodeType::Server,
        }
    }
}

impl NetGraph {
    /// Creates a new empty network graph associated with a given `node_id`.
    pub fn new(node_id: u8) -> Self {
        let graph = petgraph::graphmap::DiGraphMap::new();
        NetGraph { graph, node_id }
    }

    /// Adds a vertex to the graph if it does not already exist.
    fn save_vertices_to_graph(&mut self, vertice: Vertice) {
        if !self.graph.contains_node(vertice) {
            info!("Inserting a new vertice to graph: {vertice:?}");
            self.graph.add_node(vertice);
        }
    }

    /// Inserts a bidirectional edge between two nodes.
    fn insert_edge_between_nodes(&mut self, before: (NodeId, NodeType), after: (NodeId, NodeType)) {
        let before = Vertice::new(before);
        let after = Vertice::new(after);

        // Always add unidirectional edge from first node to latter node
        if !self.graph.contains_edge(before, after) {
            info!("Adding new edge between {before:?} and {after:?}");
            self.graph.add_edge(before, after, ());
        }

        if !self.graph.contains_edge(after, before) {
            info!("Adding new edge between {after:?} and {before:?}");
            self.graph.add_edge(after, before, ());
        }
    }

    /// Adds a new route to the graph and notifies the SC (service controller) of known topology.
    ///
    /// - Stops early if a `Client` or `Server` node appears mid-path.
    /// - Adds vertices and bidirectional edges for each step in the route.
    /// - Sends a `KnownNetworkGraph` event over the provided channel.
    pub fn add_route(
        &mut self,
        route: &[(NodeId, NodeType)],
        outbound_sc_event_channel: &Sender<NodeEvent>,
    ) -> Result<()> {
        info!("Saving a new path trace {route:?}");
        for i in 0..route.len() - 1 {
            let before = &route[i];
            let after = &route[i + 1];

            if i != route.len() - 1 {
                match after.1 {
                    NodeType::Client | NodeType::Server => return Ok(()),
                    NodeType::Drone => {}
                }
            }
            self.save_vertices_to_graph(Vertice::new(*before));
            self.save_vertices_to_graph(Vertice::new(*after));

            self.insert_edge_between_nodes(*before, *after);
        }
        self.notify_sc_of_known_topology(outbound_sc_event_channel)?;

        Ok(())
    }

    /// Returns a list of all non-drone nodes in the graph, or `None` if there are none.
    pub fn get_edge_nodes(&self) -> Option<Vec<(NodeId, NodeType)>> {
        let edge_vertices: Vec<Vertice> = self
            .graph
            .nodes()
            .filter(|x| !matches!(x.get_node_type(), NodeType::Drone))
            .collect();
        let mut nodes = vec![];
        for vertice in edge_vertices {
            nodes.push((vertice.node_id, vertice.get_node_type()));
        }
        if nodes.is_empty() { None } else { Some(nodes) }
    }

    /// Sends a `KnownNetworkGraph` event containing the current topology to the SC.
    fn notify_sc_of_known_topology(
        &self,
        outbound_sc_event_channel: &Sender<NodeEvent>,
    ) -> Result<()> {
        let mut nodes = EventNetworkGraph { nodes: vec![] };

        for node in self.graph.nodes() {
            let neighbors: Vec<Vertice> = self.graph.neighbors(node).collect();
            let neighbors = neighbors.iter().map(|node| node.node_id).collect();
            let event = EventNetworkNode {
                node_id: node.node_id,
                node_type: node.get_node_type(),
                neighbors,
            };
            nodes.nodes.push(event);
        }

        let event = NodeEvent::KnownNetworkGraph {
            source: self.node_id,
            graph: nodes,
        };
        outbound_sc_event_channel.send(event)?;
        Ok(())
    }

    /// Computes all simple routes between two vertices in the graph.
    ///
    /// Returns each route as a list of `NodeId`s.
    fn compute_routes(&self, from: Vertice, to: Vertice) -> Vec<Vec<u8>> {
        let routes: Vec<Vec<Vertice>> =
            simple_paths::all_simple_paths(&self.graph, from, to, 0, None).collect();
        routes
            .iter()
            .map(|vert_vec| vert_vec.iter().map(|vertice| vertice.node_id).collect())
            .collect()
    }

    /// Returns a random route between two vertices, or `None` if no route exists.
    pub fn get_random_route(&self, from: Vertice, to: Vertice) -> Option<Vec<u8>> {
        let routes = self.compute_routes(from, to);
        if routes.is_empty() {
            return None;
        }
        let random_index = rng().random_range(0..routes.len());
        routes.get(random_index).cloned()
    }

    /// Returns the `NodeType` for a node ID if it exists in the graph.
    ///
    /// Returns an error if the node is not found.
    pub fn get_node_type(&self, from: u8) -> Result<NodeType> {
        self.graph.visit_map();
        let node_type: Vec<Vertice> = self
            .graph
            .nodes()
            .filter(|node| node.node_id == from)
            .collect();
        Ok(node_type
            .first()
            .with_context(|| "Queried for node type but did not find such node from graph!")?
            .get_node_type())
    }

    /// Clears all vertices and edges from the graph.
    pub fn reset(&mut self) {
        self.graph.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use wg_2024::packet::NodeType;

    fn v(id: u8, t: NodeType) -> Vertice {
        Vertice::new((id, t))
    }

    #[test]
    fn test_vertice_new_and_type_mapping() {
        let v1 = v(1, NodeType::Client);
        assert_eq!(v1.get_node_type(), NodeType::Client);

        let v2 = v(2, NodeType::Drone);
        assert_eq!(v2.get_node_type(), NodeType::Drone);

        let v3 = v(3, NodeType::Server);
        assert_eq!(v3.get_node_type(), NodeType::Server);
    }

    #[test]
    fn test_save_vertices_to_graph_inserts_once() {
        let mut graph = NetGraph::new(0);
        let vtx = v(1, NodeType::Drone);
        graph.save_vertices_to_graph(vtx);
        assert!(graph.graph.contains_node(vtx));

        // Try inserting same vertex again
        graph.save_vertices_to_graph(vtx);
        assert_eq!(graph.graph.nodes().count(), 1);
    }

    #[test]
    fn test_insert_edge_between_nodes_bidirectional() {
        let mut graph = NetGraph::new(0);
        graph.insert_edge_between_nodes((1, NodeType::Drone), (2, NodeType::Drone));
        assert!(
            graph
                .graph
                .contains_edge(v(1, NodeType::Drone), v(2, NodeType::Drone))
        );
        assert!(
            graph
                .graph
                .contains_edge(v(2, NodeType::Drone), v(1, NodeType::Drone))
        );
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn test_add_route_stops_on_client_or_server() {
        let mut graph = NetGraph::new(0);
        let (tx, _rx) = unbounded();

        // Middle node is a Client — should early return without inserting edges
        graph
            .add_route(&[(1, NodeType::Drone), (2, NodeType::Client)], &tx)
            .unwrap();
        assert!(graph.graph.nodes().count() <= 1);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn test_get_edge_nodes_filters_drones() {
        let mut graph = NetGraph::new(0);
        graph.save_vertices_to_graph(v(1, NodeType::Drone));
        graph.save_vertices_to_graph(v(2, NodeType::Server));

        let nodes = graph.get_edge_nodes().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0], (2, NodeType::Server));
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn test_compute_and_random_routes() {
        let mut graph = NetGraph::new(0);
        graph.insert_edge_between_nodes((1, NodeType::Drone), (2, NodeType::Drone));
        graph.insert_edge_between_nodes((2, NodeType::Drone), (3, NodeType::Drone));

        let all_routes = graph.compute_routes(v(1, NodeType::Drone), v(3, NodeType::Drone));
        assert_eq!(all_routes, vec![vec![1, 2, 3]]);

        let random_route = graph.get_random_route(v(1, NodeType::Drone), v(3, NodeType::Drone));
        assert!(random_route.is_some());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn test_get_node_type_existing_and_missing() {
        let mut graph = NetGraph::new(0);
        graph.save_vertices_to_graph(v(5, NodeType::Server));
        assert_eq!(graph.get_node_type(5).unwrap(), NodeType::Server);

        let result = graph.get_node_type(99);
        assert!(result.is_err());
    }
}
