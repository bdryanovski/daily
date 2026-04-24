# Storage Markdown

> A file-system-backed `Storage` implementation that persists every node as a markdown file with
> YAML frontmatter, organized into folders by node type.

---

## What

Today, `SqliteStorage` is the only `Storage` implementation and acts as the single source of truth.
This change introduces `storage_markdown` — a new crate that implements the `Storage` trait by
reading/writing markdown files on disk.

**Markdown becomes the source of truth.** SQLite's role shifts to search and navigation index only;
it can always be rebuilt from the markdown files. This spec covers only the `storage_markdown` crate
itself — the SQLite sync/rebuild mechanism is out of scope.

## Requirements

| ID   | Requirement                                                                                                 | Priority |
| ---- | ----------------------------------------------------------------------------------------------------------- | -------- |
| R-1  | Implement the `Storage` trait (`insert_node`, `get_node`, `update_node`, `delete_node`)                     | Must     |
| R-2  | Persist each node as a single `.md` file with YAML frontmatter for metadata and markdown body for `content` | Must     |
| R-3  | Organize files into subfolders by `NodeType` (e.g. `tasks/`, `notes/`, `journal/`, `habits/`)               | Must     |
| R-4  | Root storage path must be configurable via constructor parameter                                            | Must     |
| R-5  | Create type folders automatically on first write                                                            | Must     |
| R-6  | Round-trip fidelity — reading a written node must produce the same `Node` (no data loss)                    | Must     |
| R-7  | File names must be unique and deterministic given a node ID                                                 | Must     |
| R-8  | Handle `NodeType::Custom(name)` by using the custom name as the folder                                      | Must     |
| R-10 | Fix `NodeType::as_str()` so `Custom(name)` returns `name` instead of `"custom"`                             | Must     |
| R-11 | Maintain an in-memory `HashMap<NodeId, PathBuf>` index, built on startup and kept in sync on writes         | Must     |
| R-9  | Sanitize folder/file names to avoid invalid filesystem characters                                           | Should   |

## Design

### File Layout

```
<root>/
  tasks/
    <node-id>.md
  habits/
    <node-id>.md
  journal/
    <node-id>.md
  notes/
    <node-id>.md
  <custom-type>/
    <node-id>.md
```

Files are named by full UUID (`<node-id>.md`). This guarantees uniqueness without dealing with title
slugification edge cases (collisions, renames, encoding). Titles live in frontmatter.

### Markdown File Format

```markdown
---
id: 'd4f5a6b7-1234-5678-9abc-def012345678'
title: 'Build something meaningful'
type: 'task'
attributes:
  priority: 'high'
  energy: 8
created_at: '2025-04-24T10:30:00Z'
updated_at: '2025-04-24T11:00:00Z'
---

This is the node content as markdown body.
```

- **Frontmatter**: YAML between `---` delimiters. Contains `id`, `title`, `type`, `attributes`,
  `created_at`, `updated_at`.
- **Body**: Everything after the closing `---`. Maps to `Node.content`. If `content` is `None`, body
  is empty.
- `attributes` is serialized as a YAML mapping (not a JSON string).

### Key Decisions

| Decision                       | Choice                                                      | Rationale                                                                                                                 |
| ------------------------------ | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| File naming                    | UUID-based (`<id>.md`)                                      | Avoids slug collisions, rename complexity, and encoding issues. Simple, deterministic.                                    |
| Frontmatter format             | YAML                                                        | Widely supported, human-readable, standard for markdown files (Jekyll, Obsidian, Hugo).                                   |
| Folder per type                | `NodeType::as_str()` as folder name                         | Direct mapping, easy to browse. `Custom("foo")` → `foo/`. Requires fixing `as_str()` to return inner name for `Custom`.   |
| Path lookup                    | In-memory `HashMap<NodeId, PathBuf>` built at startup       | O(1) lookups for `get`/`update`/`delete`. Avoids scanning folders on every read.                                          |
| Locking strategy               | No locking                                                  | Each node is a separate file — concurrent writes to different nodes don't conflict. Cross-process safety is out of scope. |
| Content `None` vs empty string | `None` → no body (or empty), empty string `""` → empty body | On read: missing/empty body → `None`.                                                                                     |

### Architecture / Data Flow

```
Engine
  └─ Storage (trait)
       ├─ SqliteStorage   (existing — future: index/search only)
       └─ MarkdownStorage (new — source of truth)
              └─ in-memory HashMap<NodeId, PathBuf>
```

#### `new(root)`

1. Create root if missing (`fs::create_dir_all`)
2. Walk `<root>/*/*.md`, parse each file's frontmatter `id`, populate the `HashMap<NodeId, PathBuf>`
3. Return `MarkdownStorage` with the populated index
4. Corrupt files: log and skip (do not fail startup)

#### `insert_node`

1. Determine folder: `<root>/<node_type.as_str()>/`
2. Create folder if it doesn't exist (`fs::create_dir_all`)
3. Build frontmatter YAML from node metadata
4. Write `<root>/<type>/<id>.md`
5. Fail if ID is already in the index (duplicate insert)
6. Insert `(id, path)` into the in-memory index

#### `get_node`

1. Look up path in the in-memory index → `NotFound` if missing
2. Read file, parse frontmatter → metadata, body → `content`

#### `update_node`

1. Look up existing path in the index → `NotFound` if missing
2. If `node_type` changed: delete old file, write to new type folder, update index entry
3. Otherwise: overwrite in place with updated frontmatter + body
4. Set `updated_at` to now

#### `delete_node`

1. Look up path in the index → `NotFound` if missing
2. Delete the file
3. Remove entry from the index

### Crate Structure

```
crates/storage_markdown/
  Cargo.toml
  src/
    lib.rs        — MarkdownStorage struct + Storage impl
    format.rs     — serialize/deserialize Node ↔ markdown+frontmatter
```

### Dependencies

```toml
[dependencies]
core = { path = "../core" }
domain = { path = "../domain" }
storage = { path = "../storage" }
serde_yaml = "0.9"
chrono = { version = "0.4", features = ["serde"] }
serde_json = "1"
```

### Interface Changes

New public struct:

```rust
pub struct MarkdownStorage {
    root: PathBuf,
}

impl MarkdownStorage {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self>;
}

impl Storage for MarkdownStorage { ... }
```

No changes to existing traits, structs, or crates. Drop-in replacement for `SqliteStorage`.

## Error Handling

| Scenario                                                      | Expected Behavior                                                         |
| ------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Root directory doesn't exist                                  | `new()` creates it via `create_dir_all`, or returns `CoreError::Storage`  |
| Insert with duplicate ID (file exists)                        | Return `CoreError::Storage("node already exists")`                        |
| `get_node` / `delete_node` for nonexistent ID                 | Return `CoreError::NotFound`                                              |
| Frontmatter parse failure (corrupt file)                      | Return `CoreError::Serialization` with details                            |
| Filesystem permission error                                   | Return `CoreError::Storage` wrapping the IO error                         |
| `NodeType::Custom` with invalid folder chars (e.g. `/`, `\0`) | Sanitize: replace invalid chars with `_`, return error if result is empty |

## Testing Strategy

1. **Unit tests** (`format.rs`):
   - Round-trip: `Node` → markdown string → `Node` (all fields preserved)
   - Frontmatter with all attribute types (string, number, bool, null, nested)
   - `content: None` produces empty body; empty body reads back as `None`
   - Edge cases: title with special YAML chars, multiline content

2. **Integration tests** (`lib.rs` or `tests/`):
   - CRUD lifecycle: insert → get → update → get → delete → get (NotFound)
   - Files appear in correct type folders
   - Duplicate insert returns error
   - Delete nonexistent returns NotFound
   - Node type change moves file to new folder
   - `Custom("my-type")` creates `my-type/` folder

3. **Use temp directories** (`tempfile` crate) for all filesystem tests.

### Required Change in `domain` Crate

Fix `NodeType::as_str()` to return the inner name for `Custom`:

```rust
// After — return type changes from &'static str to &str
pub fn as_str(&self) -> &str {
    match self {
        NodeType::Task => "task",
        NodeType::Habit => "habit",
        NodeType::Journal => "journal",
        NodeType::Note => "note",
        NodeType::Custom(name) => name,
    }
}
```

**Impact**: Return type changes from `&'static str` to `&str`. Existing caller in `storage_sqlite`
(`node.node_type.as_str()` inside `insert_node`) continues to work since `rusqlite::params!` accepts
`&str`. `NodeType::from_string` already handles round-trip correctly (unknown strings become
`Custom(other)`).

## Out of Scope

- SQLite sync/rebuild from markdown files (separate spec)
- Cross-process file locking
- File watching / live reload
- Filename slugification by title (using UUID only)
- Migration from existing SQLite data to markdown
- Listing/querying nodes (that's the index crate's job)

## Open Questions

- [x] **File lookup performance**: In-memory `HashMap<NodeId, PathBuf>` built at startup.
      _(Decided.)_
- [x] **`NodeType::Custom` loses the custom name**: Fix `NodeType::as_str()` to return the inner
      string. _(Decided.)_
- [x] **Concurrent writes**: No locking — each node is a separate file, so writes to different nodes
      are independent. _(Decided.)_
