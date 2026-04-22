# LifeTrack — Core Library Specification

> A personal productivity core: habits, tasks, and journal entries in one library.  
> Build once, run everywhere — CLI, TUI, REST, Web, iOS.

---

## Table of Contents

1. [Language Choice](#1-language-choice)
2. [Project Structure](#2-project-structure)
3. [Domain Model](#3-domain-model)
4. [Storage Architecture](#4-storage-architecture)
5. [SQLite Backend](#5-sqlite-backend)
6. [Markdown / Filesystem Backend](#6-markdown--filesystem-backend)
7. [Core API Design](#7-core-api-design)
8. [Query & Filter System](#8-query--filter-system)
9. [Sync & Conflict Resolution](#9-sync--conflict-resolution)
10. [FFI — Swift Bindings](#10-ffi--swift-bindings)
11. [FFI — TypeScript / WASM Bindings](#11-ffi--typescript--wasm-bindings)
12. [Plugin Interface Layer](#12-plugin-interface-layer)
13. [Error Handling Strategy](#13-error-handling-strategy)
14. [Testing Strategy](#14-testing-strategy)
15. [Roadmap & Milestones](#15-roadmap--milestones)

---

## 1. Language Choice

### Recommendation: Rust

Both Rust and Go are solid choices, but for your specific goals (iOS Swift wrapper, WASM/TypeScript
bindings, performance, embedded SQLite) **Rust is the stronger pick**. Here's the honest comparison:

| Concern           | Rust                                                   | Go                                                             |
| ----------------- | ------------------------------------------------------ | -------------------------------------------------------------- |
| C FFI for Swift   | Excellent — `cbindgen` generates headers automatically | Possible with `cgo`, but awkward and adds Go runtime to binary |
| WASM              | First-class via `wasm-pack` / `wasm-bindgen`           | Experimental, large binary size                                |
| Embedded SQLite   | `rusqlite` is mature and well-maintained               | `mattn/go-sqlite3` requires CGO — cross-compilation pain       |
| iOS static lib    | `cargo build --target aarch64-apple-ios` just works    | Requires Go mobile toolchain, limited                          |
| Binary size       | Small static libs, no GC                               | Larger (runtime included)                                      |
| Async             | `tokio` is excellent for REST layer                    | Goroutines are simpler and more ergonomic                      |
| Learning curve    | Steeper (borrow checker)                               | Gentler                                                        |
| Filesystem safety | Ownership model prevents file handle bugs              | Garbage collected, less control                                |

**Verdict:** Use Rust for the core library. For the REST API layer specifically, you can still use
Go if you prefer by calling the Rust library via FFI from Go — but keeping everything in Rust is
simpler.

The borrow checker will feel painful at first but it perfectly models the problem of "one piece of
data, multiple UIs consuming it concurrently" — exactly your use case.

---

## 2. Project Structure

```
lifetrack/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── lifetrack-core/         # ★ main library — pure domain logic
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── domain/         # types: Habit, Task, Entry, Tag
│   │   │   ├── storage/        # trait + backends
│   │   │   ├── query/          # filter/sort/pagination
│   │   │   ├── sync/           # conflict resolution
│   │   │   └── error.rs
│   │   └── Cargo.toml
│   │
│   ├── lifetrack-sqlite/       # SQLite storage backend
│   │   └── src/lib.rs
│   │
│   ├── lifetrack-fs/           # Markdown/filesystem backend
│   │   └── src/lib.rs
│   │
│   ├── lifetrack-ffi/          # C FFI layer for Swift
│   │   ├── src/lib.rs
│   │   ├── build.rs            # runs cbindgen
│   │   └── lifetrack.h         # generated C header
│   │
│   ├── lifetrack-wasm/         # WASM + JS bindings
│   │   └── src/lib.rs
│   │
│   └── lifetrack-cli/          # reference CLI (also dogfoods the API)
│       └── src/main.rs
│
├── bindings/
│   ├── swift/                  # Swift package wrapping the C FFI
│   │   ├── Package.swift
│   │   └── Sources/LifeTrack/
│   └── typescript/             # TypeScript types + WASM loader
│       ├── package.json
│       └── src/
│
└── docs/
    └── spec.md                 # this document
```

### Workspace `Cargo.toml`

```toml
[workspace]
members = [
    "crates/lifetrack-core",
    "crates/lifetrack-sqlite",
    "crates/lifetrack-fs",
    "crates/lifetrack-ffi",
    "crates/lifetrack-wasm",
    "crates/lifetrack-cli",
]
resolver = "2"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
thiserror = "1"
anyhow = "1"
tokio = { version = "1", features = ["full"] }
```

---

## 3. Domain Model

These are the canonical types that live in `lifetrack-core`. Every backend must map to/from these.

### 3.1 Core Identifiers

```rust
// crates/lifetrack-core/src/domain/id.rs
use uuid::Uuid;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Id(Uuid);

impl Id {
    pub fn new() -> Self { Id(Uuid::new_v4()) }
    pub fn from_str(s: &str) -> Result<Self, uuid::Error> {
        Ok(Id(Uuid::parse_str(s)?))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

### 3.2 Habit

A recurring behavior tracked over time. The core entity for streak logic.

```rust
// crates/lifetrack-core/src/domain/habit.rs
use chrono::{NaiveDate, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};
use super::Id;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Habit {
    pub id: Id,
    pub title: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub frequency: Frequency,
    pub target: HabitTarget,
    pub reminder: Option<Reminder>,
    pub archived: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Frequency {
    Daily,
    Weekly { days: Vec<Weekday> },
    Monthly { days: Vec<u8> },         // day-of-month 1..=31
    Interval { every_n_days: u32 },
    Custom { cron: String },            // for power users
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HabitTarget {
    Boolean,                            // did it or didn't
    Count { goal: u32, unit: String },  // e.g. 8 glasses of water
    Duration { goal_minutes: u32 },     // e.g. 30 min exercise
    Numeric { goal: f64, unit: String, direction: Direction }, // weight, steps
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Direction { AtLeast, AtMost, Exactly }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HabitEntry {
    pub id: Id,
    pub habit_id: Id,
    pub date: NaiveDate,
    pub value: HabitValue,
    pub note: Option<String>,
    pub logged_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HabitValue {
    Done,
    Skipped,
    Count(u32),
    Duration(u32),  // minutes
    Numeric(f64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub time: NaiveTime,
    pub message: Option<String>,
}
```

### 3.3 Task

A one-off or recurring action item with optional project/area grouping.

```rust
// crates/lifetrack-core/src/domain/task.rs
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use super::Id;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Id,
    pub title: String,
    pub body: Option<String>,          // markdown content
    pub status: TaskStatus,
    pub priority: Priority,
    pub tags: Vec<String>,
    pub project_id: Option<Id>,
    pub area_id: Option<Id>,
    pub parent_id: Option<Id>,         // subtasks
    pub due_date: Option<NaiveDate>,
    pub scheduled_date: Option<NaiveDate>,
    pub completed_at: Option<DateTime<Utc>>,
    pub recurrence: Option<TaskRecurrence>,
    pub checklist: Vec<ChecklistItem>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Inbox,
    Todo,
    InProgress,
    Waiting,       // blocked on something external
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum Priority { Low, Medium, High, Critical }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub id: Id,
    pub title: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecurrence {
    pub frequency: crate::domain::habit::Frequency,
    pub create_on_completion: bool, // spawn next instance when this one is done
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
    pub title: String,
    pub description: Option<String>,
    pub area_id: Option<Id>,
    pub color: Option<String>,   // hex
    pub icon: Option<String>,
    pub status: ProjectStatus,
    pub due_date: Option<NaiveDate>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProjectStatus { Active, OnHold, Completed, Cancelled }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Area {
    pub id: Id,
    pub title: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### 3.4 Journal Entry

Free-form daily notes with optional structured fields.

```rust
// crates/lifetrack-core/src/domain/journal.rs
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use super::Id;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: Id,
    pub date: NaiveDate,
    pub title: Option<String>,
    pub body: String,              // raw markdown
    pub mood: Option<Mood>,
    pub energy: Option<u8>,        // 1–10
    pub tags: Vec<String>,
    pub linked_tasks: Vec<Id>,
    pub linked_habits: Vec<Id>,
    pub template_id: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mood {
    pub score: i8,    // -5 to +5
    pub label: Option<String>, // "anxious", "calm", "excited" etc.
    pub emoji: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalTemplate {
    pub id: Id,
    pub name: String,
    pub body: String,  // markdown with {{placeholders}}
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
}
```

### 3.5 Tag System

Tags are first-class. They live independently so they can carry metadata.

```rust
// crates/lifetrack-core/src/domain/tag.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub slug: String,   // URL-safe, e.g. "work", "morning-routine"
    pub label: String,  // display name
    pub color: Option<String>,
    pub description: Option<String>,
}
```

---

## 4. Storage Architecture

The key abstraction is a trait. The rest of the library never touches a backend directly — it only
speaks the trait. This is what allows you to swap SQLite for Markdown seamlessly.

```rust
// crates/lifetrack-core/src/storage/mod.rs
use async_trait::async_trait;
use crate::domain::*;
use crate::query::Query;
use crate::error::LtError;

pub type Result<T> = std::result::Result<T, LtError>;

/// The single trait every storage backend must implement.
/// All methods are async — even the filesystem backend uses tokio::fs.
#[async_trait]
pub trait Store: Send + Sync {
    // --- Habits ---
    async fn habit_create(&self, habit: Habit) -> Result<Habit>;
    async fn habit_get(&self, id: &Id) -> Result<Option<Habit>>;
    async fn habit_update(&self, habit: Habit) -> Result<Habit>;
    async fn habit_delete(&self, id: &Id) -> Result<()>;
    async fn habit_list(&self, query: &Query<Habit>) -> Result<Page<Habit>>;

    async fn entry_log(&self, entry: HabitEntry) -> Result<HabitEntry>;
    async fn entry_get(&self, habit_id: &Id, date: NaiveDate) -> Result<Option<HabitEntry>>;
    async fn entry_range(&self, habit_id: &Id, from: NaiveDate, to: NaiveDate) -> Result<Vec<HabitEntry>>;

    // --- Tasks ---
    async fn task_create(&self, task: Task) -> Result<Task>;
    async fn task_get(&self, id: &Id) -> Result<Option<Task>>;
    async fn task_update(&self, task: Task) -> Result<Task>;
    async fn task_delete(&self, id: &Id) -> Result<()>;
    async fn task_list(&self, query: &Query<Task>) -> Result<Page<Task>>;

    async fn project_create(&self, project: Project) -> Result<Project>;
    async fn project_get(&self, id: &Id) -> Result<Option<Project>>;
    async fn project_list(&self, query: &Query<Project>) -> Result<Page<Project>>;

    // --- Journal ---
    async fn entry_create(&self, entry: JournalEntry) -> Result<JournalEntry>;
    async fn entry_get_by_date(&self, date: NaiveDate) -> Result<Option<JournalEntry>>;
    async fn entry_update(&self, entry: JournalEntry) -> Result<JournalEntry>;
    async fn entry_delete(&self, id: &Id) -> Result<()>;
    async fn entry_search(&self, query: &Query<JournalEntry>) -> Result<Page<JournalEntry>>;

    // --- Cross-cutting ---
    async fn tags_all(&self) -> Result<Vec<Tag>>;
    async fn stats_habits(&self, habit_id: &Id, range: DateRange) -> Result<HabitStats>;

    // Backend identity — useful for UI to show "SQLite" vs "iCloud"
    fn backend_name(&self) -> &'static str;
    fn is_readonly(&self) -> bool { false }
}

/// Paginated result wrapper
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Clone)]
pub struct DateRange {
    pub from: chrono::NaiveDate,
    pub to: chrono::NaiveDate,
}
```

### 4.1 Computed Statistics

Stats are computed on top of raw entries — this logic lives in `core` and doesn't need to be
reimplemented per backend.

```rust
// crates/lifetrack-core/src/domain/stats.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HabitStats {
    pub habit_id: Id,
    pub range: DateRange,
    pub completion_rate: f64,       // 0.0 - 1.0
    pub current_streak: u32,
    pub longest_streak: u32,
    pub total_completions: u32,
    pub total_scheduled: u32,
    pub daily_values: Vec<(chrono::NaiveDate, Option<HabitValue>)>,
}

impl HabitStats {
    /// Compute stats from raw entries. Call this from the Store impl or above it.
    pub fn compute(habit: &Habit, entries: &[HabitEntry], range: DateRange) -> Self {
        // streak computation, completion rate, etc.
        // This stays in core so all backends share the same logic.
        todo!()
    }
}
```

---

## 5. SQLite Backend

The SQLite backend lives in `lifetrack-sqlite` and implements `Store`. It uses `rusqlite` with WAL
mode for performance and concurrent reads.

### 5.1 Setup

```toml
# crates/lifetrack-sqlite/Cargo.toml
[dependencies]
lifetrack-core = { path = "../lifetrack-core" }
rusqlite = { version = "0.31", features = ["bundled", "chrono", "uuid"] }
tokio = { version = "1", features = ["rt-multi-thread"] }
async-trait = "0.1"
serde_json = "1"
thiserror = "1"
```

> Use `features = ["bundled"]` so rusqlite compiles SQLite from source — no system dependency
> needed. This is important for cross-compilation to iOS.

### 5.2 Database Initialization

```rust
// crates/lifetrack-sqlite/src/lib.rs
use rusqlite::{Connection, params};
use std::path::Path;

pub struct SqliteStore {
    // Connection wrapped in a mutex because rusqlite Connection is not Send.
    // For high-concurrency scenarios, use a connection pool (r2d2 or deadpool).
    conn: tokio::sync::Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        // Enable WAL for concurrent reads without blocking writes
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let store = Self { conn: tokio::sync::Mutex::new(conn) };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let store = Self { conn: tokio::sync::Mutex::new(conn) };
        store.run_migrations()?;
        Ok(store)
    }

    fn run_migrations(&self) -> Result<(), rusqlite::Error> {
        // Embed migrations as const strings — no migration library needed for personal projects
        const MIGRATIONS: &[&str] = &[
            include_str!("migrations/001_initial.sql"),
            include_str!("migrations/002_journal.sql"),
        ];
        // Track applied migrations in a meta table
        todo!()
    }
}
```

### 5.3 Schema

```sql
-- crates/lifetrack-sqlite/src/migrations/001_initial.sql

CREATE TABLE IF NOT EXISTS habits (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    description TEXT,
    tags        TEXT NOT NULL DEFAULT '[]',   -- JSON array
    frequency   TEXT NOT NULL,                -- JSON
    target      TEXT NOT NULL,                -- JSON
    reminder    TEXT,                         -- JSON or NULL
    archived    INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS habit_entries (
    id          TEXT PRIMARY KEY,
    habit_id    TEXT NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
    date        TEXT NOT NULL,               -- YYYY-MM-DD
    value       TEXT NOT NULL,               -- JSON
    note        TEXT,
    logged_at   TEXT NOT NULL,
    UNIQUE(habit_id, date)
);
CREATE INDEX idx_habit_entries_habit_date ON habit_entries(habit_id, date);

CREATE TABLE IF NOT EXISTS tasks (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    body            TEXT,
    status          TEXT NOT NULL DEFAULT 'Inbox',
    priority        TEXT NOT NULL DEFAULT 'Medium',
    tags            TEXT NOT NULL DEFAULT '[]',
    project_id      TEXT REFERENCES projects(id) ON DELETE SET NULL,
    area_id         TEXT REFERENCES areas(id) ON DELETE SET NULL,
    parent_id       TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    due_date        TEXT,
    scheduled_date  TEXT,
    completed_at    TEXT,
    recurrence      TEXT,                    -- JSON or NULL
    checklist       TEXT NOT NULL DEFAULT '[]', -- JSON array
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_due_date ON tasks(due_date);
CREATE INDEX idx_tasks_project ON tasks(project_id);

CREATE TABLE IF NOT EXISTS projects (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    description TEXT,
    area_id     TEXT REFERENCES areas(id) ON DELETE SET NULL,
    color       TEXT,
    icon        TEXT,
    status      TEXT NOT NULL DEFAULT 'Active',
    due_date    TEXT,
    tags        TEXT NOT NULL DEFAULT '[]',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS areas (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    description TEXT,
    color       TEXT,
    icon        TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
```

```sql
-- crates/lifetrack-sqlite/src/migrations/002_journal.sql

CREATE TABLE IF NOT EXISTS journal_entries (
    id          TEXT PRIMARY KEY,
    date        TEXT NOT NULL UNIQUE,         -- YYYY-MM-DD, one entry per day
    title       TEXT,
    body        TEXT NOT NULL DEFAULT '',
    mood        TEXT,                         -- JSON or NULL
    energy      INTEGER,                      -- 1-10
    tags        TEXT NOT NULL DEFAULT '[]',
    linked_tasks    TEXT NOT NULL DEFAULT '[]',
    linked_habits   TEXT NOT NULL DEFAULT '[]',
    template_id TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX idx_journal_date ON journal_entries(date);

CREATE VIRTUAL TABLE IF NOT EXISTS journal_fts
    USING fts5(body, title, tokenize='porter unicode61');
-- Keep FTS in sync via triggers
CREATE TRIGGER journal_fts_insert AFTER INSERT ON journal_entries BEGIN
    INSERT INTO journal_fts(rowid, body, title) VALUES (new.rowid, new.body, new.title);
END;
CREATE TRIGGER journal_fts_update AFTER UPDATE ON journal_entries BEGIN
    UPDATE journal_fts SET body=new.body, title=new.title WHERE rowid=old.rowid;
END;
CREATE TRIGGER journal_fts_delete AFTER DELETE ON journal_entries BEGIN
    DELETE FROM journal_fts WHERE rowid=old.rowid;
END;

CREATE TABLE IF NOT EXISTS journal_templates (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    body        TEXT NOT NULL,
    tags        TEXT NOT NULL DEFAULT '[]',
    created_at  TEXT NOT NULL
);
```

### 5.4 Example Implementation

```rust
// Implementing one method to show the pattern
#[async_trait]
impl Store for SqliteStore {
    async fn habit_create(&self, habit: Habit) -> Result<Habit> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO habits (id, title, description, tags, frequency, target, reminder, archived, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                habit.id.to_string(),
                habit.title,
                habit.description,
                serde_json::to_string(&habit.tags)?,
                serde_json::to_string(&habit.frequency)?,
                serde_json::to_string(&habit.target)?,
                habit.reminder.as_ref().map(|r| serde_json::to_string(r)).transpose()?,
                habit.archived as i32,
                habit.created_at.to_rfc3339(),
                habit.updated_at.to_rfc3339(),
            ],
        ).map_err(LtError::from)?;
        Ok(habit)
    }

    async fn task_list(&self, query: &Query<Task>) -> Result<Page<Task>> {
        let conn = self.conn.lock().await;
        // Build WHERE clause dynamically from the Query object (see §8)
        let (where_sql, params) = query.to_sql();
        let sql = format!(
            "SELECT * FROM tasks {where_sql} ORDER BY {} {} LIMIT {} OFFSET {}",
            query.sort_field(), query.sort_dir(), query.limit, query.offset
        );
        // ... map rows to Task structs
        todo!()
    }

    fn backend_name(&self) -> &'static str { "sqlite" }
}
```

---

## 6. Markdown / Filesystem Backend

This backend stores each entity as a Markdown file with YAML front-matter — identical to how
Obsidian stores notes. It makes iCloud sync completely natural since iCloud syncs files, not
databases.

### 6.1 File Layout

```
~/Library/Mobile Documents/iCloud~com~lifetrack/Documents/
├── habits/
│   ├── _index.json              # lightweight index for fast listing
│   ├── morning-run.md
│   └── read-10-pages.md
├── habit-entries/
│   ├── 2025/
│   │   ├── 01/
│   │   │   ├── morning-run.json   # daily entry files grouped by month
│   │   │   └── read-10-pages.json
├── tasks/
│   ├── _index.json
│   ├── 2025-01-15-fix-auth-bug.md
│   └── projects/
│       └── lifetrack-v1.md
├── journal/
│   ├── 2025/
│   │   ├── 2025-01-15.md
│   │   └── 2025-01-16.md
│   └── templates/
│       └── daily.md
└── .lifetrack/
    └── config.json
```

### 6.2 Markdown File Format

Every entity uses YAML front-matter. The body is freeform markdown — this is what the user sees in
Obsidian.

```markdown
---
id: 550e8400-e29b-41d4-a716-446655440000
type: habit
title: Morning Run
frequency:
  type: weekly
  days: [Mon, Wed, Fri]
target:
  type: duration
  goal_minutes: 30
tags: [health, morning]
archived: false
created_at: '2025-01-01T07:00:00Z'
updated_at: '2025-01-15T08:30:00Z'
---

Go outside and run at least 30 minutes. Focus on pace not distance.

## Notes

Started this habit after reading _Atomic Habits_. Key insight: attach it to existing morning coffee
routine.
```

```markdown
---
id: 661f9500-f39c-52e5-b827-557766550001
type: journal
date: '2025-01-15'
mood:
  score: 3
  label: energized
energy: 8
tags: [productive, focused]
linked_tasks:
  - 550e8400-e29b-41d4-a716-446655440001
---

## Morning Check-in

Woke up at 6:30am, completed the run. Feeling good.

## Evening Review

Shipped the auth fix. Tomorrow: tackle the dashboard layout.
```

### 6.3 Filesystem Store Implementation

```rust
// crates/lifetrack-fs/src/lib.rs
use std::path::PathBuf;
use tokio::fs;
use gray_matter::{Matter, engine::YAML};  // "gray-matter" crate for front-matter parsing

pub struct FsStore {
    root: PathBuf,
    matter: Matter<YAML>,
}

impl FsStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            matter: Matter::<YAML>::new(),
        }
    }

    fn habit_path(&self, slug: &str) -> PathBuf {
        self.root.join("habits").join(format!("{slug}.md"))
    }

    fn journal_path(&self, date: chrono::NaiveDate) -> PathBuf {
        self.root
            .join("journal")
            .join(date.format("%Y").to_string())
            .join(format!("{}.md", date.format("%Y-%m-%d")))
    }

    async fn write_habit(&self, habit: &Habit) -> Result<()> {
        let slug = slugify(&habit.title);
        let path = self.habit_path(&slug);
        fs::create_dir_all(path.parent().unwrap()).await?;
        let content = render_habit_to_markdown(habit);
        fs::write(path, content).await?;
        self.update_index("habits", habit.id.clone(), &slug).await?;
        Ok(())
    }

    async fn update_index(&self, dir: &str, id: Id, slug: &str) -> Result<()> {
        // Maintains _index.json for O(1) id → filename lookup
        // without scanning the whole directory
        let index_path = self.root.join(dir).join("_index.json");
        let mut index: serde_json::Map<String, serde_json::Value> =
            if index_path.exists() {
                serde_json::from_str(&fs::read_to_string(&index_path).await?)?
            } else {
                Default::default()
            };
        index.insert(id.to_string(), serde_json::Value::String(slug.to_string()));
        fs::write(index_path, serde_json::to_string_pretty(&index)?).await?;
        Ok(())
    }
}

fn render_habit_to_markdown(habit: &Habit) -> String {
    // Serialize front-matter as YAML, append body
    format!(
        "---\n{yaml}---\n\n{body}",
        yaml = serde_yaml::to_string(habit).unwrap(),
        body = habit.description.as_deref().unwrap_or(""),
    )
}
```

### 6.4 Dual-Store (SQLite + FS Mirror)

For the best of both worlds — fast queries from SQLite, iCloud-syncable files as source of truth —
implement a `MirrorStore`:

```rust
// crates/lifetrack-core/src/storage/mirror.rs

/// Writes to both backends. Reads from SQLite (fast queries).
/// On startup, can reconcile FS into SQLite for conflict resolution.
pub struct MirrorStore {
    primary: Arc<dyn Store>,   // SQLite — for reads
    mirror: Arc<dyn Store>,    // FsStore — for iCloud sync
}

#[async_trait]
impl Store for MirrorStore {
    async fn habit_create(&self, habit: Habit) -> Result<Habit> {
        let result = self.primary.habit_create(habit.clone()).await?;
        // Mirror write is best-effort — don't fail if FS is unavailable
        if let Err(e) = self.mirror.habit_create(habit).await {
            tracing::warn!("Mirror write failed: {e}");
        }
        Ok(result)
    }

    async fn habit_list(&self, query: &Query<Habit>) -> Result<Page<Habit>> {
        // Fast path: always read from SQLite
        self.primary.habit_list(query).await
    }

    // ...
}

impl MirrorStore {
    /// Call on app launch to import any FS changes (e.g. edits made in Obsidian)
    pub async fn reconcile(&self) -> Result<ReconcileReport> {
        todo!()  // see §9 for conflict resolution
    }
}
```

---

## 7. Core API Design

The `Store` trait is low-level. Build a `LifeTrack` facade on top — the actual public API that CLI,
REST, and UI code will call.

```rust
// crates/lifetrack-core/src/lib.rs

pub struct LifeTrack {
    store: Arc<dyn Store>,
}

impl LifeTrack {
    pub fn new(store: impl Store + 'static) -> Self {
        Self { store: Arc::new(store) }
    }

    // --- Habits ---

    pub async fn create_habit(&self, req: CreateHabitRequest) -> Result<Habit> {
        let now = chrono::Utc::now();
        let habit = Habit {
            id: Id::new(),
            title: req.title,
            description: req.description,
            tags: req.tags.unwrap_or_default(),
            frequency: req.frequency,
            target: req.target,
            reminder: req.reminder,
            archived: false,
            created_at: now,
            updated_at: now,
        };
        self.store.habit_create(habit).await
    }

    pub async fn log_habit(&self, req: LogHabitRequest) -> Result<HabitEntry> {
        let habit = self.store.habit_get(&req.habit_id).await?
            .ok_or(LtError::NotFound(req.habit_id.to_string()))?;
        // Validate value matches habit target type
        validate_habit_value(&habit.target, &req.value)?;
        let entry = HabitEntry {
            id: Id::new(),
            habit_id: req.habit_id,
            date: req.date.unwrap_or_else(|| chrono::Utc::now().date_naive()),
            value: req.value,
            note: req.note,
            logged_at: chrono::Utc::now(),
        };
        self.store.entry_log(entry).await
    }

    pub async fn habit_streak(&self, habit_id: &Id) -> Result<StreakInfo> {
        let today = chrono::Utc::now().date_naive();
        let from = today - chrono::Duration::days(365);
        let entries = self.store.entry_range(habit_id, from, today).await?;
        let habit = self.store.habit_get(habit_id).await?
            .ok_or(LtError::NotFound(habit_id.to_string()))?;
        Ok(compute_streak(&habit, &entries, today))
    }

    // --- Tasks ---

    pub async fn create_task(&self, req: CreateTaskRequest) -> Result<Task> {
        // If task has recurrence, create the first instance
        todo!()
    }

    pub async fn complete_task(&self, id: &Id) -> Result<Task> {
        let mut task = self.store.task_get(id).await?
            .ok_or(LtError::NotFound(id.to_string()))?;
        task.status = TaskStatus::Done;
        task.completed_at = Some(chrono::Utc::now());
        task.updated_at = chrono::Utc::now();

        // If recurring, spawn next instance
        if let Some(recurrence) = &task.recurrence.clone() {
            if recurrence.create_on_completion {
                let next = next_recurring_task(&task)?;
                self.store.task_create(next).await?;
            }
        }
        self.store.task_update(task).await
    }

    // --- Journal ---

    pub async fn today_entry(&self) -> Result<Option<JournalEntry>> {
        let today = chrono::Utc::now().date_naive();
        self.store.entry_get_by_date(today).await
    }

    pub async fn write_journal(&self, req: WriteJournalRequest) -> Result<JournalEntry> {
        let date = req.date.unwrap_or_else(|| chrono::Utc::now().date_naive());
        match self.store.entry_get_by_date(date).await? {
            Some(mut existing) => {
                existing.body = req.body;
                existing.mood = req.mood.or(existing.mood);
                existing.energy = req.energy.or(existing.energy);
                existing.tags.extend(req.tags.unwrap_or_default());
                existing.tags.dedup();
                existing.updated_at = chrono::Utc::now();
                self.store.entry_update(existing).await
            }
            None => {
                let entry = JournalEntry {
                    id: Id::new(),
                    date,
                    title: req.title,
                    body: req.body,
                    mood: req.mood,
                    energy: req.energy,
                    tags: req.tags.unwrap_or_default(),
                    linked_tasks: vec![],
                    linked_habits: vec![],
                    template_id: req.template_id,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                };
                self.store.entry_create(entry).await
            }
        }
    }
}
```

### 7.1 Request / Response Types

Keep request types separate from domain types — they're the public API surface.

```rust
// crates/lifetrack-core/src/api/requests.rs
#[derive(Debug, serde::Deserialize)]
pub struct CreateHabitRequest {
    pub title: String,
    pub description: Option<String>,
    pub frequency: Frequency,
    pub target: HabitTarget,
    pub tags: Option<Vec<String>>,
    pub reminder: Option<Reminder>,
}

#[derive(Debug, serde::Deserialize)]
pub struct LogHabitRequest {
    pub habit_id: Id,
    pub value: HabitValue,
    pub date: Option<chrono::NaiveDate>,
    pub note: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    pub body: Option<String>,
    pub priority: Option<Priority>,
    pub tags: Option<Vec<String>>,
    pub project_id: Option<Id>,
    pub area_id: Option<Id>,
    pub parent_id: Option<Id>,
    pub due_date: Option<chrono::NaiveDate>,
    pub scheduled_date: Option<chrono::NaiveDate>,
    pub recurrence: Option<TaskRecurrence>,
    pub checklist: Option<Vec<String>>,  // just titles — IDs auto-generated
}

#[derive(Debug, serde::Deserialize)]
pub struct WriteJournalRequest {
    pub body: String,
    pub date: Option<chrono::NaiveDate>,
    pub title: Option<String>,
    pub mood: Option<Mood>,
    pub energy: Option<u8>,
    pub tags: Option<Vec<String>>,
    pub template_id: Option<Id>,
}
```

---

## 8. Query & Filter System

A type-safe query builder that each backend can translate to its native query language (SQL or file
iteration).

```rust
// crates/lifetrack-core/src/query/mod.rs
use std::marker::PhantomData;

#[derive(Debug, Clone)]
pub struct Query<T> {
    pub filters: Vec<Filter>,
    pub sort: Sort,
    pub limit: usize,
    pub offset: usize,
    _phantom: PhantomData<T>,
}

impl<T> Query<T> {
    pub fn new() -> Self {
        Self {
            filters: vec![],
            sort: Sort::default(),
            limit: 50,
            offset: 0,
            _phantom: PhantomData,
        }
    }

    pub fn filter(mut self, f: Filter) -> Self {
        self.filters.push(f);
        self
    }

    pub fn sort_by(mut self, field: &str, dir: SortDir) -> Self {
        self.sort = Sort { field: field.to_string(), dir };
        self
    }

    pub fn paginate(mut self, limit: usize, offset: usize) -> Self {
        self.limit = limit;
        self.offset = offset;
        self
    }
}

#[derive(Debug, Clone)]
pub enum Filter {
    TagIn(Vec<String>),
    StatusIn(Vec<String>),          // Task status
    PriorityGte(String),
    DueBefore(chrono::NaiveDate),
    DueAfter(chrono::NaiveDate),
    ProjectId(Id),
    AreaId(Id),
    TextSearch(String),             // FTS for journal, title search for tasks
    Archived(bool),
    And(Vec<Filter>),
    Or(Vec<Filter>),
}

#[derive(Debug, Clone)]
pub struct Sort {
    pub field: String,
    pub dir: SortDir,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortDir { Asc, Desc }

impl Default for Sort {
    fn default() -> Self { Sort { field: "created_at".to_string(), dir: SortDir::Desc } }
}
```

### 8.1 SQL Translation

```rust
// crates/lifetrack-sqlite/src/query.rs
impl Filter {
    /// Translate a filter into a SQL fragment and its params.
    /// Call recursively for And/Or.
    pub fn to_sql(&self) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
        match self {
            Filter::StatusIn(statuses) => {
                let placeholders = statuses.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                let params: Vec<Box<dyn rusqlite::ToSql>> = statuses.iter()
                    .map(|s| Box::new(s.clone()) as _)
                    .collect();
                (format!("status IN ({placeholders})"), params)
            }
            Filter::TagIn(tags) => {
                // JSON array contains check using SQLite json_each
                let conditions: Vec<String> = tags.iter()
                    .map(|t| format!("EXISTS(SELECT 1 FROM json_each(tags) WHERE value='{t}')"))
                    .collect();
                (format!("({})", conditions.join(" OR ")), vec![])
            }
            Filter::DueBefore(date) => {
                (format!("due_date <= '{}'", date.format("%Y-%m-%d")), vec![])
            }
            Filter::TextSearch(q) => {
                // Use FTS for journal, LIKE for tasks
                (format!("title LIKE '%{q}%'"), vec![])
            }
            Filter::And(filters) => {
                let (clauses, params): (Vec<_>, Vec<_>) = filters.iter()
                    .map(|f| f.to_sql())
                    .unzip();
                (format!("({})", clauses.join(" AND ")), params.into_iter().flatten().collect())
            }
            _ => todo!("implement remaining filters")
        }
    }
}
```

### 8.2 Example Query Usage

```rust
// From any consumer (CLI, REST, etc.)
let query = Query::<Task>::new()
    .filter(Filter::StatusIn(vec!["Todo".into(), "InProgress".into()]))
    .filter(Filter::TagIn(vec!["work".into()]))
    .filter(Filter::DueBefore(chrono::NaiveDate::from_ymd_opt(2025, 2, 1).unwrap()))
    .sort_by("due_date", SortDir::Asc)
    .paginate(20, 0);

let page = lt.store.task_list(&query).await?;
println!("Found {} tasks (total: {})", page.items.len(), page.total);
```

---

## 9. Sync & Conflict Resolution

When running with `MirrorStore` (SQLite + iCloud FS), you need to handle the case where a file was
edited externally (e.g. in Obsidian on another device).

### 9.1 Conflict Detection Strategy

```rust
// crates/lifetrack-core/src/sync/mod.rs

#[derive(Debug)]
pub enum ConflictResolution {
    /// FS file is newer → import into SQLite
    FsWins,
    /// SQLite record is newer → re-export to FS
    SqliteWins,
    /// Same updated_at, different content → create conflict copy
    CreateConflict,
}

pub async fn reconcile_habit(
    fs_habit: &Habit,
    db_habit: Option<&Habit>,
) -> ConflictResolution {
    match db_habit {
        None => ConflictResolution::FsWins,  // new file appeared
        Some(db) => {
            if fs_habit.updated_at > db.updated_at {
                ConflictResolution::FsWins
            } else if db.updated_at > fs_habit.updated_at {
                ConflictResolution::SqliteWins
            } else {
                // Same timestamp — compare content hash
                let fs_hash = content_hash(fs_habit);
                let db_hash = content_hash(db);
                if fs_hash == db_hash {
                    ConflictResolution::SqliteWins  // identical, no-op
                } else {
                    ConflictResolution::CreateConflict
                }
            }
        }
    }
}
```

### 9.2 Conflict Copies

Inspired by iCloud Drive's own conflict file naming:

```
journal/2025/2025-01-15.md
journal/2025/2025-01-15 (conflict 2025-01-20T14:32:00).md
```

The user can resolve manually in any text editor or Obsidian.

---

## 10. FFI — Swift Bindings

The FFI layer exposes a C-compatible API that Swift can consume via a static library + header file.

### 10.1 Setup

```toml
# crates/lifetrack-ffi/Cargo.toml
[lib]
crate-type = ["staticlib", "cdylib"]

[dependencies]
lifetrack-core = { path = "../lifetrack-core" }
lifetrack-sqlite = { path = "../lifetrack-sqlite" }
tokio = { version = "1", features = ["rt"] }
serde_json = "1"

[build-dependencies]
cbindgen = "0.27"
```

```rust
// crates/lifetrack-ffi/build.rs
fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(cbindgen::Language::C)
        .generate()
        .expect("Unable to generate C bindings")
        .write_to_file("lifetrack.h");
}
```

### 10.2 C API Pattern

The pattern: pass JSON strings across the FFI boundary. It's simpler than mapping every struct to C,
and JSON parsing on both sides is trivial. This is what many production SDK FFI layers do.

```rust
// crates/lifetrack-ffi/src/lib.rs
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// Global tokio runtime — one per process
static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().unwrap())
}

// Opaque handle to a LifeTrack instance
pub struct LtHandle {
    inner: lifetrack_core::LifeTrack,
}

/// Create a new LifeTrack instance backed by SQLite at `db_path`.
/// Returns NULL on failure.
/// Caller must free with `lt_destroy`.
#[no_mangle]
pub extern "C" fn lt_create_sqlite(db_path: *const c_char) -> *mut LtHandle {
    let path = unsafe { CStr::from_ptr(db_path) }.to_str().unwrap_or_default();
    match lifetrack_sqlite::SqliteStore::open(path) {
        Ok(store) => {
            let lt = lifetrack_core::LifeTrack::new(store);
            Box::into_raw(Box::new(LtHandle { inner: lt }))
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a LifeTrack handle.
#[no_mangle]
pub extern "C" fn lt_destroy(handle: *mut LtHandle) {
    if !handle.is_null() {
        unsafe { drop(Box::from_raw(handle)); }
    }
}

/// Create a habit. `request_json` is a JSON-encoded CreateHabitRequest.
/// Returns a JSON-encoded Habit string. Caller must free with `lt_free_string`.
/// Returns NULL on error.
#[no_mangle]
pub extern "C" fn lt_habit_create(
    handle: *mut LtHandle,
    request_json: *const c_char,
) -> *mut c_char {
    let handle = unsafe { &*handle };
    let json = unsafe { CStr::from_ptr(request_json) }.to_str().unwrap_or_default();

    let req: lifetrack_core::CreateHabitRequest = match serde_json::from_str(json) {
        Ok(r) => r,
        Err(_) => return std::ptr::null_mut(),
    };

    let result = runtime().block_on(handle.inner.create_habit(req));
    match result {
        Ok(habit) => {
            let json = serde_json::to_string(&habit).unwrap();
            CString::new(json).unwrap().into_raw()
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a string returned by the library.
#[no_mangle]
pub extern "C" fn lt_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)); }
    }
}
```

### 10.3 Swift Package

```swift
// bindings/swift/Sources/LifeTrack/LifeTrack.swift
import Foundation

// Link the generated header
// In Package.swift: target depends on the binary target wrapping lifetrack.a

public class LifeTrack {
    private let handle: OpaquePointer

    public init(dbPath: String) throws {
        guard let h = lt_create_sqlite(dbPath) else {
            throw LifeTrackError.initFailed
        }
        handle = h
    }

    deinit { lt_destroy(handle) }

    public func createHabit(_ request: CreateHabitRequest) throws -> Habit {
        let json = try JSONEncoder().encode(request)
        let jsonStr = String(data: json, encoding: .utf8)!
        guard let resultPtr = lt_habit_create(handle, jsonStr) else {
            throw LifeTrackError.operationFailed
        }
        defer { lt_free_string(resultPtr) }
        let resultStr = String(cString: resultPtr)
        return try JSONDecoder().decode(Habit.self, from: resultStr.data(using: .utf8)!)
    }
}

// Swift-side Codable mirrors of domain types
public struct CreateHabitRequest: Codable {
    public let title: String
    public let frequency: Frequency
    public let target: HabitTarget
    public var description: String?
    public var tags: [String]?
    public var reminder: Reminder?
}
```

```swift
// bindings/swift/Package.swift
// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "LifeTrack",
    platforms: [.iOS(.v16), .macOS(.v13)],
    products: [.library(name: "LifeTrack", targets: ["LifeTrack"])],
    targets: [
        // Pre-built static library + generated header
        .binaryTarget(
            name: "lifetrack_ffi",
            path: "Frameworks/lifetrack_ffi.xcframework"
        ),
        .target(
            name: "LifeTrack",
            dependencies: ["lifetrack_ffi"],
            path: "Sources/LifeTrack"
        ),
    ]
)
```

### 10.4 Build Script for iOS

```bash
#!/bin/bash
# scripts/build-ios.sh

# Build for iOS device (arm64) and simulator (x86_64 + arm64)
cargo build --release --target aarch64-apple-ios \
    -p lifetrack-ffi

cargo build --release --target x86_64-apple-ios \
    -p lifetrack-ffi

cargo build --release --target aarch64-apple-ios-sim \
    -p lifetrack-ffi

# Create XCFramework for distribution
xcodebuild -create-xcframework \
    -library target/aarch64-apple-ios/release/liblifetrack_ffi.a \
    -headers crates/lifetrack-ffi/lifetrack.h \
    -library target/aarch64-apple-ios-sim/release/liblifetrack_ffi.a \
    -headers crates/lifetrack-ffi/lifetrack.h \
    -output bindings/swift/Frameworks/lifetrack_ffi.xcframework
```

---

## 11. FFI — TypeScript / WASM Bindings

### 11.1 Setup

```toml
# crates/lifetrack-wasm/Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
lifetrack-core = { path = "../lifetrack-core" }
lifetrack-sqlite = { path = "../lifetrack-sqlite" }
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
serde-wasm-bindgen = "0.6"
```

### 11.2 WASM Bindings

```rust
// crates/lifetrack-wasm/src/lib.rs
use wasm_bindgen::prelude::*;
use lifetrack_core::{LifeTrack, CreateHabitRequest};

#[wasm_bindgen]
pub struct WasmLifeTrack {
    inner: LifeTrack,
}

#[wasm_bindgen]
impl WasmLifeTrack {
    /// Initialize with an in-memory SQLite database (for web).
    /// For Electron/Tauri, pass a real path via the FS backend.
    #[wasm_bindgen(constructor)]
    pub async fn new() -> Result<WasmLifeTrack, JsValue> {
        let store = lifetrack_sqlite::SqliteStore::open_in_memory()
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(WasmLifeTrack { inner: LifeTrack::new(store) })
    }

    /// All methods accept and return plain JS objects (via serde-wasm-bindgen).
    #[wasm_bindgen(js_name = createHabit)]
    pub async fn create_habit(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let req: CreateHabitRequest = serde_wasm_bindgen::from_value(request)?;
        let habit = self.inner.create_habit(req).await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(serde_wasm_bindgen::to_value(&habit)?)
    }

    #[wasm_bindgen(js_name = logHabit)]
    pub async fn log_habit(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let req = serde_wasm_bindgen::from_value(request)?;
        let entry = self.inner.log_habit(req).await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(serde_wasm_bindgen::to_value(&entry)?)
    }
}
```

### 11.3 TypeScript Package

```typescript
// bindings/typescript/src/index.ts
import init, { WasmLifeTrack } from '../wasm/lifetrack_wasm';

export * from './types'; // Re-export generated TypeScript types

let _db: WasmLifeTrack | null = null;

export async function initLifeTrack(): Promise<void> {
  await init(); // Load .wasm file
  _db = await new WasmLifeTrack();
}

function db(): WasmLifeTrack {
  if (!_db) throw new Error('Call initLifeTrack() first');
  return _db;
}

export const habits = {
  create: (req: CreateHabitRequest) => db().createHabit(req),
  list: (query?: QueryOptions) => db().listHabits(query ?? {}),
  log: (req: LogHabitRequest) => db().logHabit(req),
  streak: (habitId: string) => db().habitStreak(habitId),
};

export const tasks = {
  create: (req: CreateTaskRequest) => db().createTask(req),
  complete: (id: string) => db().completeTask(id),
  list: (query?: QueryOptions) => db().listTasks(query ?? {}),
};

export const journal = {
  write: (req: WriteJournalRequest) => db().writeJournal(req),
  today: () => db().todayEntry(),
  search: (text: string) => db().searchJournal(text),
};
```

```typescript
// bindings/typescript/src/types.ts
// Hand-authored TypeScript mirrors — keep in sync with Rust types.
// Consider using typeshare (https://github.com/1Password/typeshare) to auto-generate these.

export interface Habit {
  id: string;
  title: string;
  description?: string;
  tags: string[];
  frequency: Frequency;
  target: HabitTarget;
  reminder?: Reminder;
  archived: boolean;
  createdAt: string; // ISO8601
  updatedAt: string;
}

export type Frequency =
  | { type: 'Daily' }
  | { type: 'Weekly'; days: string[] }
  | { type: 'Monthly'; days: number[] }
  | { type: 'Interval'; everyNDays: number };

export type HabitTarget =
  | { type: 'Boolean' }
  | { type: 'Count'; goal: number; unit: string }
  | { type: 'Duration'; goalMinutes: number }
  | { type: 'Numeric'; goal: number; unit: string; direction: 'AtLeast' | 'AtMost' | 'Exactly' };

export interface Task {
  id: string;
  title: string;
  body?: string;
  status: 'Inbox' | 'Todo' | 'InProgress' | 'Waiting' | 'Done' | 'Cancelled';
  priority: 'Low' | 'Medium' | 'High' | 'Critical';
  tags: string[];
  projectId?: string;
  dueDate?: string; // YYYY-MM-DD
  completedAt?: string;
  checklist: ChecklistItem[];
  createdAt: string;
  updatedAt: string;
}
```

> **Tip:** Use [`typeshare`](https://github.com/1Password/typeshare) to auto-generate TypeScript
> types from your Rust types. Add `#[typeshare]` to your domain structs and run
> `typeshare ./crates/lifetrack-core --lang=typescript --output-file=bindings/typescript/src/types.ts`.

---

## 12. Plugin Interface Layer

Consumers of the core library (REST, TUI, CLI) each live in their own crate/package and only depend
on `lifetrack-core`.

### 12.1 REST API (Axum)

```rust
// A separate crate: lifetrack-rest/src/main.rs
use axum::{Router, routing::{get, post, put, delete}, Json, extract::{State, Path}};
use lifetrack_core::{LifeTrack, CreateHabitRequest};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let store = lifetrack_sqlite::SqliteStore::open("data.db").unwrap();
    let lt = Arc::new(LifeTrack::new(store));

    let app = Router::new()
        .route("/habits",           post(create_habit).get(list_habits))
        .route("/habits/:id",       get(get_habit).put(update_habit).delete(delete_habit))
        .route("/habits/:id/log",   post(log_habit))
        .route("/habits/:id/streak",get(habit_streak))
        .route("/tasks",            post(create_task).get(list_tasks))
        .route("/tasks/:id/complete", post(complete_task))
        .route("/journal",          post(write_journal).get(list_journal))
        .route("/journal/today",    get(today_entry))
        .with_state(lt);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn create_habit(
    State(lt): State<Arc<LifeTrack>>,
    Json(req): Json<CreateHabitRequest>,
) -> Result<Json<Habit>, StatusCode> {
    lt.create_habit(req).await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
```

### 12.2 CLI (Clap)

```rust
// lifetrack-cli/src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "lt", about = "LifeTrack — personal productivity tracker")]
struct Cli {
    #[arg(long, env = "LT_DB", default_value = "~/.lifetrack/data.db")]
    db: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Habit management
    Habit {
        #[command(subcommand)]
        action: HabitAction,
    },
    /// Task management
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// Journal
    Journal {
        #[command(subcommand)]
        action: JournalAction,
    },
}

#[derive(Subcommand)]
enum HabitAction {
    /// Create a new habit
    Create { title: String, #[arg(long)] daily: bool },
    /// Log today's habit
    Log { habit: String, #[arg(long)] note: Option<String> },
    /// Show streaks
    Streaks,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let store = lifetrack_sqlite::SqliteStore::open(&cli.db).unwrap();
    let lt = lifetrack_core::LifeTrack::new(store);

    match cli.command {
        Commands::Habit { action } => handle_habit(lt, action).await,
        Commands::Task  { action } => handle_task(lt, action).await,
        Commands::Journal { action } => handle_journal(lt, action).await,
    }
}
```

### 12.3 TUI (Ratatui)

```rust
// lifetrack-tui/src/app.rs — sketch of state machine
use ratatui::{Terminal, Frame};

pub enum Screen { Dashboard, Habits, Tasks, Journal, HabitDetail(Id) }

pub struct App {
    lt: Arc<LifeTrack>,
    screen: Screen,
    habits: Vec<Habit>,
    tasks: Vec<Task>,
}

impl App {
    pub async fn run(&mut self, terminal: &mut Terminal<impl Backend>) {
        loop {
            terminal.draw(|f| self.render(f)).unwrap();
            if let Event::Key(key) = event::read().unwrap() {
                match key.code {
                    KeyCode::Char('h') => self.screen = Screen::Habits,
                    KeyCode::Char('t') => self.screen = Screen::Tasks,
                    KeyCode::Char('j') => self.screen = Screen::Journal,
                    KeyCode::Char('q') => break,
                    _ => self.handle_input(key).await,
                }
            }
        }
    }
}
```

---

## 13. Error Handling Strategy

Centralize all errors in `lifetrack-core`. Every backend maps its native errors to `LtError`.

```rust
// crates/lifetrack-core/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LtError {
    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Conflict: {local} vs {remote}")]
    SyncConflict { local: String, remote: String },
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite: {0}")]
    Sqlite(String),

    #[error("Filesystem: {0}")]
    Fs(String),

    #[error("Migration failed: {0}")]
    Migration(String),
}
```

---

## 14. Testing Strategy

### 14.1 Unit Tests (in core)

```rust
// crates/lifetrack-core/src/domain/tests.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streak_resets_on_missed_day() {
        let habit = Habit { frequency: Frequency::Daily, .. test_habit() };
        let today = chrono::NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
        // Entry exists for Jan 13 and 15, but NOT Jan 14 — streak should be 1
        let entries = vec![
            test_entry(today - chrono::Duration::days(2)),
            test_entry(today),
        ];
        let streak = compute_streak(&habit, &entries, today);
        assert_eq!(streak.current, 1);
    }
}
```

### 14.2 Backend Integration Tests

```rust
// crates/lifetrack-sqlite/tests/integration.rs
use lifetrack_core::{LifeTrack, CreateHabitRequest, Frequency, HabitTarget};

#[tokio::test]
async fn test_create_and_retrieve_habit() {
    let store = lifetrack_sqlite::SqliteStore::open_in_memory().unwrap();
    let lt = LifeTrack::new(store);

    let habit = lt.create_habit(CreateHabitRequest {
        title: "Morning Run".to_string(),
        frequency: Frequency::Daily,
        target: HabitTarget::Boolean,
        description: None,
        tags: None,
        reminder: None,
    }).await.unwrap();

    let fetched = lt.store.habit_get(&habit.id).await.unwrap().unwrap();
    assert_eq!(fetched.title, "Morning Run");
}

#[tokio::test]
async fn test_fs_backend_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = lifetrack_fs::FsStore::new(dir.path());
    let lt = LifeTrack::new(store);
    // Same test — backend is swappable
    // ...
}
```

### 14.3 FFI Smoke Test

```bash
# Compile the C test against the static library
clang -o test_ffi tests/ffi_test.c \
    -I crates/lifetrack-ffi/lifetrack.h \
    -L target/release -l lifetrack_ffi
./test_ffi
```

---

## 15. Roadmap & Milestones

### Phase 1 — Core Library (Weeks 1–3)

- [ ] Domain types: `Habit`, `Task`, `JournalEntry`, and friends
- [ ] `Store` trait defined
- [ ] SQLite backend with migrations
- [ ] `LifeTrack` facade with `create_habit`, `log_habit`, `create_task`, `complete_task`,
      `write_journal`
- [ ] Basic query/filter system
- [ ] Unit tests for streak computation

### Phase 2 — CLI + FS Backend (Weeks 4–5)

- [ ] Filesystem backend (read/write markdown files)
- [ ] `MirrorStore` dual-write
- [ ] `lifetrack-cli` with habit logging, task management, journal writing
- [ ] Basic reconciliation on startup

### Phase 3 — REST + TUI (Weeks 6–8)

- [ ] Axum REST API with full CRUD + query
- [ ] Ratatui TUI with dashboard, habit tracker, task list, journal
- [ ] OpenAPI spec (using `utoipa`)

### Phase 4 — Swift Bindings + iOS (Weeks 9–11)

- [ ] C FFI layer with `cbindgen`
- [ ] XCFramework build script
- [ ] Swift Package with Codable wrappers
- [ ] Basic SwiftUI app consuming the library

### Phase 5 — WASM + Web (Weeks 12–14)

- [ ] `wasm-pack` build
- [ ] TypeScript bindings with `typeshare` types
- [ ] Simple web UI (SvelteKit or React)
- [ ] PWA with offline capability via OPFS (Origin Private File System for SQLite in browser)

### Phase 6 — Polish & Sync (Ongoing)

- [ ] Full conflict resolution UI in iOS and TUI
- [ ] Notification/reminder system (platform-specific)
- [ ] Import from Obsidian, Notion, Todoist
- [ ] Encryption at rest (for journal)
- [ ] Plugin API for custom dashboards and stats

---

## Key Dependencies Summary

| Crate                  | Purpose                             |
| ---------------------- | ----------------------------------- |
| `rusqlite` + `bundled` | SQLite — embedded, no system dep    |
| `serde` + `serde_json` | Serialization everywhere            |
| `chrono`               | Date/time types                     |
| `uuid`                 | Entity IDs                          |
| `thiserror`            | Error types                         |
| `async-trait`          | Async in trait definitions          |
| `tokio`                | Async runtime                       |
| `cbindgen`             | Generate C headers from Rust        |
| `wasm-bindgen`         | WASM + JS interop                   |
| `serde-wasm-bindgen`   | JS↔Rust value conversion            |
| `axum`                 | REST API                            |
| `clap`                 | CLI arg parsing                     |
| `ratatui`              | TUI framework                       |
| `gray-matter`          | Parse YAML front-matter in markdown |
| `serde_yaml`           | YAML serialization for FS backend   |
| `typeshare`            | Generate TypeScript types from Rust |
| `tracing`              | Structured logging                  |
| `tempfile`             | Temp dirs in tests                  |

---

_This document is the single source of truth for the LifeTrack library architecture. Update it as
design decisions evolve._
