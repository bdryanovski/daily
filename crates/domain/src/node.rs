use chrono::{DateTime, Utc};
use core::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeType {
    Task,
    Habit,
    Journal,
    Note,
    Custom(String),
}

impl NodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::Task => "task",
            NodeType::Habit => "habit",
            NodeType::Journal => "journal",
            NodeType::Note => "note",
            NodeType::Custom(_) => "custom",
        }
    }

    pub fn from_string(string: &str) -> Self {
        match string {
            "task" => NodeType::Task,
            "habit" => NodeType::Habit,
            "journal" => NodeType::Journal,
            "note" => NodeType::Note,
            other => NodeType::Custom(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub node_type: NodeType,
    pub title: String,
    pub content: Option<String>,
    pub attributes: HashMap<String, serde_json::Value>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl Node {
    pub fn new(title: impl Into<String>, node_type: NodeType) -> Self {
        Self {
            id: NodeId::new(),
            node_type,
            title: title.into(),
            content: None,
            attributes: HashMap::new(),
            created_at: None,
            updated_at: None,
        }
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn set_attr(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.attributes.insert(key.into(), value);
    }

    pub fn get_attr(&self, key: &str) -> Option<&serde_json::Value> {
        self.attributes.get(key)
    }

    pub fn with_type(mut self, node_type: NodeType) -> Self {
        self.node_type = node_type;
        self
    }
}
