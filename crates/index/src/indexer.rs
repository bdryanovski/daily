use crate::node_index::NodeIndex;
use core::NodeId;

pub trait Index {
    fn get(&self, id: &NodeId) -> Option<&NodeIndex>;
    fn all(&self) -> Vec<&NodeIndex>;
    fn insert(&mut self, node: NodeIndex);
    fn remove(&mut self, id: &NodeId);
}
