use core::{NodeId, Result};
use domain::{Node, NodeType};
use storage::Storage;

pub struct Engine<S: Storage> {
    storage: S,
}

impl<S: Storage> Engine<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    pub fn create_node(&self, title: &str, t: NodeType) -> Result<Node> {
        let node = Node::new(title, t);
        self.storage.insert_node(&node)?;
        Ok(node)
    }

    pub fn get_node(&self, id: &NodeId) -> Result<Node> {
        self.storage.get_node(id)
    }

    pub fn update_node(&self, node: &Node) -> Result<()> {
        self.storage.update_node(node)
    }

    pub fn delete_node(&self, id: &NodeId) -> Result<()> {
        self.storage.delete_node(id)
    }
}
