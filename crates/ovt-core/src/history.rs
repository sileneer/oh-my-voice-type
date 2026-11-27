//! Local SQLite history repository. Migrations are additive so rows written by
//! older releases remain searchable and keep their original ordering.

use crate::config::HistorySettings;
use crate::error::Result;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub created_at_ms: u64,
    pub mode: String,
    pub app_name: Option<String>,
    pub raw_text: String,
    pub final_text: String,
    pub asr_ms: Option<u64>,
    pub cleanup_ms: Option<u64>,
    pub insert_ok: bool,
    pub pinned: bool,
    pub target_language: Option<String>,
    /// Relative to the application's audio directory.
    pub audio_path: Option<String>,
    pub delivery_status: String,
    pub warnings_json: String,
    pub source_pending_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryPage {
    pub entries: Vec<HistoryEntry>,
    pub total: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryFilter {
    pub query: Option<String>,
    pub mode: Option<String>,
    pub delivery: Option<String>,
    pub pinned: Option<bool>,
    /// Inclusive Unix timestamp in milliseconds.
    pub from_ms: Option<u64>,
    /// Exclusive Unix timestamp in milliseconds.
    pub to_ms: Option<u64>,
    pub limit: u32,
    pub offset: u32,
    /// Strict newest-first order (e.g. a "recent" list) instead of pinned first.
    pub newest_first: bool,
}

pub struct HistoryRepo {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for HistoryRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryRepo").finish_non_exhaustive()
    }
}

const COLUMNS: &str = "id, created_at_ms, mode, app_name, raw_text, final_text, asr_ms, cleanup_ms, insert_ok, pinned, target_language, audio_path, delivery_status, warnings_json, source_pending_id";

impl HistoryRepo {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| crate::Error::Db(format!("create history dir: {e}")))?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id TEXT PRIMARY KEY,
                created_at_ms INTEGER NOT NULL,
                mode TEXT NOT NULL,
                app_name TEXT,
                raw_text TEXT NOT NULL,
                final_text TEXT NOT NULL,
                asr_ms INTEGER,
                cleanup_ms INTEGER,
                insert_ok INTEGER NOT NULL DEFAULT 0,
                pinned INTEGER NOT NULL DEFAULT 0,
                target_language TEXT,
                audio_path TEXT,
                delivery_status TEXT NOT NULL DEFAULT 'failed',
                warnings_json TEXT NOT NULL DEFAULT '[]',
                source_pending_id TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_history_created ON history(created_at_ms DESC);",
        )?;
        let existing: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(history)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (name, definition) in [
            ("target_language", "TEXT"),
            ("audio_path", "TEXT"),
            ("delivery_status", "TEXT NOT NULL DEFAULT 'failed'"),
            ("warnings_json", "TEXT NOT NULL DEFAULT '[]'"),
            ("source_pending_id", "TEXT"),
        ] {
            if !existing.iter().any(|column| column == name) {
                conn.execute_batch(&format!(
                    "ALTER TABLE history ADD COLUMN {name} {definition};"
                ))?;
                if name == "delivery_status" {
                    conn.execute_batch(
                        "UPDATE history SET delivery_status = 'inserted' WHERE insert_ok = 1;",
                    )?;
                }
            }
        }
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_history_pending
             ON history(source_pending_id) WHERE source_pending_id IS NOT NULL;",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, entry: &HistoryEntry) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO history
             (id, created_at_ms, mode, app_name, raw_text, final_text, asr_ms, cleanup_ms, insert_ok, pinned,
              target_language, audio_path, delivery_status, warnings_json, source_pending_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                entry.id,
                entry.created_at_ms as i64,
                entry.mode,
                entry.app_name,
                entry.raw_text,
                entry.final_text,
                entry.asr_ms.map(|v| v as i64),
                entry.cleanup_ms.map(|v| v as i64),
                entry.insert_ok as i64,
                entry.pinned as i64,
                entry.target_language,
                entry.audio_path,
                entry.delivery_status,
                entry.warnings_json,
                entry.source_pending_id
            ],
        )?;
        Ok(())
    }

    pub fn list(&self, limit: u32, offset: u32, query: Option<&str>) -> Result<HistoryPage> {
        self.list_filtered(&HistoryFilter {
            query: query.map(str::to_string),
            limit,
            offset,
            ..HistoryFilter::default()
        })
    }

    pub fn list_filtered(&self, filter: &HistoryFilter) -> Result<HistoryPage> {
        let conn = self.conn.lock().unwrap();
        let mut clauses = Vec::new();
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(query) = filter
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
        {
            clauses.push("(final_text LIKE ? OR raw_text LIKE ?)");
            let like = format!("%{query}%");
            values.push(like.clone().into());
            values.push(like.into());
        }
        if let Some(mode) = filter.mode.as_deref().filter(|mode| !mode.is_empty()) {
            clauses.push("mode = ?");
            values.push(mode.to_string().into());
        }
        if let Some(delivery) = filter
            .delivery
            .as_deref()
            .filter(|status| !status.is_empty())
        {
            clauses.push("delivery_status = ?");
            values.push(delivery.to_string().into());
        }
        if let Some(pinned) = filter.pinned {
            clauses.push("pinned = ?");
            values.push((pinned as i64).into());
        }
        if let Some(from_ms) = filter.from_ms {
            clauses.push("created_at_ms >= ?");
            values.push((from_ms.min(i64::MAX as u64) as i64).into());
        }
        if let Some(to_ms) = filter.to_ms {
            clauses.push("created_at_ms < ?");
            values.push((to_ms.min(i64::MAX as u64) as i64).into());
        }
        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };
        let count_values = values.clone();
        let total: u64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM history{where_sql}"),
            params_from_iter(count_values.iter()),
            |row| row.get::<_, i64>(0),
        )? as u64;
        values.push((filter.limit.clamp(1, 200) as i64).into());
        values.push(filter.offset.into());
        let order = if filter.newest_first {
            "created_at_ms DESC"
        } else {
            "pinned DESC, created_at_ms DESC"
        };
        let sql =
            format!("SELECT {COLUMNS} FROM history{where_sql} ORDER BY {order} LIMIT ? OFFSET ?");
        let mut stmt = conn.prepare(&sql)?;
        let entries = stmt
            .query_map(params_from_iter(values.iter()), row_to_entry)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(HistoryPage { entries, total })
    }

    pub fn get(&self, id: &str) -> Result<Option<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM history WHERE id = ?1"),
                params![id],
                row_to_entry,
            )
            .optional()?)
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE history SET pinned = ?2 WHERE id = ?1",
            params![id, pinned as i64],
        )?;
        Ok(())
    }

    pub fn set_delivery(&self, id: &str, status: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE history SET delivery_status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM history WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn delete_with_audio(&self, id: &str) -> Result<Option<String>> {
        let audio_path = self.get(id)?.and_then(|entry| entry.audio_path);
        self.delete(id)?;
        Ok(audio_path)
    }

    pub fn audio_paths(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT audio_path FROM history WHERE audio_path IS NOT NULL")?;
        let paths = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(paths)
    }

    pub fn delete_all(&self) -> Result<Vec<String>> {
        let paths = self.audio_paths()?;
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM history", [])?;
        Ok(paths)
    }

    /// Prune entries older than the retention window (pinned entries survive).
    /// `retention_days: None` keeps everything; `Some(0)` deletes all unpinned.
    pub fn prune(&self, settings: &HistorySettings, now_ms: u64) -> Result<usize> {
        Ok(self.prune_with_audio(settings, now_ms)?.len())
    }

    pub fn prune_with_audio(&self, settings: &HistorySettings, now_ms: u64) -> Result<Vec<String>> {
        let Some(days) = settings.retention_days else {
            return Ok(Vec::new());
        };
        let cutoff = now_ms.saturating_sub(days as u64 * 24 * 60 * 60 * 1000);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT audio_path FROM history WHERE pinned = 0 AND created_at_ms < ?1 AND audio_path IS NOT NULL",
        )?;
        let paths = stmt
            .query_map(params![cutoff as i64], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        conn.execute(
            "DELETE FROM history WHERE pinned = 0 AND created_at_ms < ?1",
            params![cutoff as i64],
        )?;
        Ok(paths)
    }

    pub fn count(&self) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        let n: Option<i64> = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .optional()?;
        Ok(n.unwrap_or(0) as u64)
    }
}

fn row_to_entry(r: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: r.get(0)?,
        created_at_ms: r.get::<_, i64>(1)? as u64,
        mode: r.get(2)?,
        app_name: r.get(3)?,
        raw_text: r.get(4)?,
        final_text: r.get(5)?,
        asr_ms: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
        cleanup_ms: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        insert_ok: r.get::<_, i64>(8)? != 0,
        pinned: r.get::<_, i64>(9)? != 0,
        target_language: r.get(10)?,
        audio_path: r.get(11)?,
        delivery_status: r.get(12)?,
        warnings_json: r.get(13)?,
        source_pending_id: r.get(14)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, ts: u64) -> HistoryEntry {
        HistoryEntry {
            id: id.into(),
            created_at_ms: ts,
            mode: "dictate".into(),
            app_name: Some("Slack".into()),
            raw_text: "hello um world".into(),
            final_text: "Hello, world.".into(),
            asr_ms: Some(200),
            cleanup_ms: Some(400),
            insert_ok: true,
            pinned: false,
            delivery_status: "inserted".into(),
            warnings_json: "[]".into(),
            ..HistoryEntry::default()
        }
    }

    #[test]
    fn crud_and_search() {
        let repo = HistoryRepo::open_in_memory().unwrap();
        repo.insert(&entry("a", 1000)).unwrap();
        repo.insert(&entry("b", 2000)).unwrap();
        let page = repo.list(10, 0, None).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.entries[0].id, "b");
        assert_eq!(page.entries[0].delivery_status, "inserted");

        let hits = repo.list(10, 0, Some("um")).unwrap();
        assert_eq!(hits.total, 2);
        repo.set_pinned("a", true).unwrap();
        let page = repo.list(10, 0, None).unwrap();
        assert_eq!(page.entries[0].id, "a");
        let recent = repo
            .list_filtered(&HistoryFilter {
                limit: 1,
                newest_first: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            recent.entries[0].id, "b",
            "a pinned old entry is not recent"
        );
        repo.delete("a").unwrap();
        assert_eq!(repo.count().unwrap(), 1);
    }

    #[test]
    fn pending_source_is_unique_across_retries() {
        let repo = HistoryRepo::open_in_memory().unwrap();
        let mut first = entry("first", 1000);
        first.source_pending_id = Some("pending-1".into());
        repo.insert(&first).unwrap();
        let mut retry = entry("retry", 2000);
        retry.source_pending_id = Some("pending-1".into());
        repo.insert(&retry).unwrap();
        let page = repo.list(10, 0, None).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.entries[0].id, "retry");
    }

    #[test]
    fn filters_combine_and_paginate() {
        let repo = HistoryRepo::open_in_memory().unwrap();
        let mut translated = entry("translated", 2000);
        translated.mode = "translate".into();
        translated.target_language = Some("German".into());
        translated.delivery_status = "copied".into();
        repo.insert(&translated).unwrap();
        repo.insert(&entry("dictated", 3000)).unwrap();
        let page = repo
            .list_filtered(&HistoryFilter {
                mode: Some("translate".into()),
                delivery: Some("copied".into()),
                pinned: Some(false),
                from_ms: Some(1000),
                to_ms: Some(2500),
                limit: 50,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.entries[0].id, "translated");
        assert_eq!(
            repo.list_filtered(&HistoryFilter {
                limit: 1,
                offset: 1,
                ..Default::default()
            })
            .unwrap()
            .entries[0]
                .id,
            "translated"
        );
    }

    #[test]
    fn additive_migration_preserves_old_rows() {
        let path = std::env::temp_dir().join(format!("ovt-history-{}.db", uuid::Uuid::new_v4()));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE history (
                    id TEXT PRIMARY KEY, created_at_ms INTEGER NOT NULL, mode TEXT NOT NULL,
                    app_name TEXT, raw_text TEXT NOT NULL, final_text TEXT NOT NULL,
                    asr_ms INTEGER, cleanup_ms INTEGER, insert_ok INTEGER NOT NULL DEFAULT 0,
                    pinned INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO history VALUES ('old', 1000, 'dictate', NULL, 'raw', 'final', 1, 2, 1, 0);",
            )
            .unwrap();
        }
        let repo = HistoryRepo::open(&path).unwrap();
        let old = repo.get("old").unwrap().unwrap();
        assert_eq!(old.final_text, "final");
        assert_eq!(old.delivery_status, "inserted");
        assert_eq!(old.warnings_json, "[]");
        assert!(old.audio_path.is_none());
        drop(repo);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn prune_respects_pinned_and_returns_audio() {
        let repo = HistoryRepo::open_in_memory().unwrap();
        let mut old = entry("old", 1_000);
        old.audio_path = Some("old.wav".into());
        repo.insert(&old).unwrap();
        let mut pinned = entry("old2", 1_000);
        pinned.audio_path = Some("pinned.wav".into());
        repo.insert(&pinned).unwrap();
        repo.set_pinned("old2", true).unwrap();
        let settings = HistorySettings {
            retention_days: Some(1),
            ..Default::default()
        };
        let audio = repo.prune_with_audio(&settings, 100 * 86_400_000).unwrap();
        assert_eq!(audio, vec!["old.wav"]);
        assert_eq!(repo.count().unwrap(), 1);
    }

    #[test]
    fn repo_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<HistoryRepo>();
    }
}
