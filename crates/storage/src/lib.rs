use core::{NodeId, Result};
use domain::Node;

pub trait Storage: Send + Sync {
    fn insert_node(&self, node: &Node) -> Result<()>;
    fn get_node(&self, node: &NodeId) -> Result<Node>;
    fn update_node(&self, node: &Node) -> Result<()>;
    fn delete_node(&self, id: &NodeId) -> Result<()>;
}
