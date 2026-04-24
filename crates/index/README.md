### Example

```rust
use std::collections::HashMap;

pub struct MemoryIndex {
    nodes: HashMap<NodeId, NodeIndex>,
}

impl MemoryIndex {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }
}

impl Index for MemoryIndex {
    fn get(&self, id: &NodeId) -> Option<&NodeIndex> {
        self.nodes.get(id)
    }

    fn all(&self) -> Vec<&NodeIndex> {
        self.nodes.values().collect()
    }

    fn insert(&mut self, node: NodeIndex) {
        self.nodes.insert(node.id.clone(), node);
    }

    fn remove(&mut self, id: &NodeId) {
        self.nodes.remove(id);
    }
}
```
