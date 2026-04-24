use domain::NodeType;
use engine::Engine;
use serde_json::json;
use storage_sqlite::SqliteStorage;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let storage = SqliteStorage::new("data.db")?;
    let engine = Engine::new(storage);

    // Create
    let mut node = engine.create_node("Build something meaningful", NodeType::Task)?;

    // Enrich
    node.set_attr("priority", json!("high"));
    node.set_attr("energy", json!(8));

    engine.update_node(&node)?;

    println!("Created node: {:?}", node);

    // Fetch
    let fetched = engine.get_node(&node.id)?;
    println!("Fetched node: {:?}", fetched);

    // Delete
    engine.delete_node(&node.id)?;
    println!("Deleted node");

    Ok(())
}
