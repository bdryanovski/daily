use chrono::{DateTime, Utc};
use core::{CoreError, NodeId, Result};
use domain::{Node, NodeType};
use rusqlite::{params, Connection};
use storage::Storage;

use std::sync::Mutex;

pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| CoreError::Storage(e.to_string()))?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS nodes (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                type TEXT NOT NULL,
                content TEXT,
                attributes TEXT NOT NULL,
                created_at TEXT,
                updated_at TEXT
            );
            "#,
        )
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn serialize_attrs(node: &Node) -> Result<String> {
        serde_json::to_string(&node.attributes).map_err(|e| CoreError::Serialization(e.to_string()))
    }

    fn deserialize_attrs(
        raw: String,
    ) -> Result<std::collections::HashMap<String, serde_json::Value>> {
        serde_json::from_str(&raw).map_err(|e| CoreError::Serialization(e.to_string()))
    }
}

impl Storage for SqliteStorage {
    fn insert_node(&self, node: &Node) -> Result<()> {
        let attrs = Self::serialize_attrs(node)?;
        let conn = self.conn.lock().unwrap();

        let now: DateTime<Utc> = Utc::now();

        conn.execute(
            "INSERT INTO nodes (id, title, type, content, attributes, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                node.id.to_string(),
                node.title,
                // One way of doing it
                // serde_json::to_string(&node.node_type).unwrap(),
                node.node_type.as_str(),
                node.content,
                attrs,
                now.to_rfc3339(),
                now.to_rfc3339()
            ],
        )
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(())
    }

    fn get_node(&self, id: &NodeId) -> Result<Node> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, title, type, content, attributes, created_at, updated_at FROM nodes WHERE id = ?1")
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        /*
        Use number for performance reasons - named one is a bit slower (very little slower)
        */
        let node = stmt
            .query_row(params![id.to_string()], |row| {
                let attr_str: String = row.get(4)?;
                let attrs = Self::deserialize_attrs(attr_str).unwrap();

                Ok(Node {
                    id: NodeId::parse(&row.get::<_, String>("id")?),
                    node_type: NodeType::from_string(&row.get::<_, String>("type")?),
                    title: row.get("title")?,
                    content: row.get("content")?,
                    attributes: attrs,
                    created_at: row.get("created_at")?,
                    updated_at: row.get("updated_at")?,
                })
            })
            .map_err(|_| CoreError::NotFound)?;

        Ok(node)
    }

    fn update_node(&self, node: &Node) -> Result<()> {
        let attrs = Self::serialize_attrs(node)?;

        let conn = self.conn.lock().unwrap();

        let now: DateTime<Utc> = Utc::now();

        conn.execute(
            "UPDATE nodes SET title = ?2, content = ?3, attributes = ?4, updated_at = ?5 WHERE id = ?1",
            params![node.id.to_string(), node.title, node.content, attrs, now.to_rfc3339()],
        )
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(())
    }

    fn delete_node(&self, id: &NodeId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM nodes WHERE id = ?1", params![id.to_string()])
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(())
    }
}
