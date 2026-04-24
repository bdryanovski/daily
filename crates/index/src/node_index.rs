use core::NodeId;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct NodeIndex {
    pub id: NodeId,
    pub title: String,
    pub content: Option<String>,
    pub tags: HashSet<String>,
    pub attributes: HashMap<String, serde_json::Value>,
}
