//! SQLite-backed durable pending queue. It lives in `history.db`; audio lives
//! separately under `pending-audio/` and is referenced by relative path only.

use ovt_core::pending::{PendingStatus, PendingTask};
use ovt_core::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize)]
pub struct PendingStats {
    pub count: usize,
    pub bytes: u64,
}

pub struct PendingRepo {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for PendingRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingRepo").finish_non_exhaustive()
    }
}

impl PendingRepo {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                ovt_core::Error::Db(format!("create pending directory: {error}"))
            })?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_tasks (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                mode TEXT NOT NULL,
                audio_path TEXT NOT NULL,
                task_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_pending_status_updated
                ON pending_tasks(status, updated_at_ms);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn save(&self, task: &PendingTask) -> Result<()> {
        let json = serde_json::to_string(task)
            .map_err(|error| ovt_core::Error::Db(format!("encode pending task: {error}")))?;
        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO pending_tasks
             (id, session_id, status, created_at_ms, updated_at_ms, mode, audio_path, task_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                task.id,
                task.session_id,
                task.status.as_str(),
                task.created_at_ms as i64,
                task.updated_at_ms as i64,
                task.mode.as_str(),
                task.audio_path,
                json
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<PendingTask>> {
        let conn = self.conn.lock().unwrap();
        let json: Option<String> = conn
            .query_row(
                "SELECT task_json FROM pending_tasks WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|json| {
            serde_json::from_str(&json)
                .map_err(|error| ovt_core::Error::Db(format!("decode pending task: {error}")))
        })
        .transpose()
    }

    pub fn list(&self) -> Result<Vec<PendingTask>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT task_json FROM pending_tasks WHERE status != 'completed'
             ORDER BY created_at_ms DESC",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|json| {
            serde_json::from_str(&json?)
                .map_err(|error| ovt_core::Error::Db(format!("decode pending task: {error}")))
        })
        .collect()
    }

    pub fn transition(&self, id: &str, next: PendingStatus, now_ms: u64) -> Result<PendingTask> {
        let mut task = self
            .get(id)?
            .ok_or_else(|| ovt_core::Error::Db("pending task not found".into()))?;
        if !task.transition(next, now_ms) {
            return Err(ovt_core::Error::Db(format!(
                "invalid pending transition {} -> {}",
                task.status.as_str(),
                next.as_str()
            )));
        }
        self.save(&task)?;
        Ok(task)
    }

    pub fn delete(&self, id: &str) -> Result<Option<PendingTask>> {
        let task = self.get(id).ok().flatten();
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM pending_tasks WHERE id = ?1", params![id])?;
        Ok(task)
    }

    /// Called once at startup. No task is automatically delivered.
    pub fn recover_interrupted(&self, now_ms: u64) -> Result<usize> {
        let tasks = self.list()?;
        let mut recovered = 0;
        for mut task in tasks {
            if task.status == PendingStatus::Completed {
                continue;
            }
            let next = task.recovery_status();
            if task.status != next && task.transition(next, now_ms) {
                self.save(&task)?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    pub fn expired(&self, now_ms: u64, retention_days: u32) -> Result<Vec<PendingTask>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|task| task.expired(now_ms, retention_days))
            .collect())
    }
}

pub fn resolve_audio_path(base: &Path, relative: &str) -> Result<std::path::PathBuf> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(ovt_core::Error::Audio(
            "audio path must be a safe relative path".into(),
        ));
    }
    if !relative.starts_with("pending-audio") {
        return Err(ovt_core::Error::Audio(
            "pending audio path is outside pending-audio".into(),
        ));
    }
    Ok(base.join(relative))
}

pub fn resolve_history_audio(base: &Path, relative: &str) -> Result<std::path::PathBuf> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        || !relative.starts_with("audio")
    {
        return Err(ovt_core::Error::Audio(
            "history audio path is outside the application audio directory".into(),
        ));
    }
    Ok(base.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovt_core::config::SessionMode;

    fn task(id: &str) -> PendingTask {
        PendingTask {
            id: id.into(),
            session_id: "session".into(),
            mode: SessionMode::Dictate,
            audio_path: format!("pending-audio/{id}.wav"),
            created_at_ms: 10,
            updated_at_ms: 10,
            ..PendingTask::default()
        }
    }

    #[test]
    fn persists_transitions_and_recovers_without_delivery() {
        let path = std::env::temp_dir().join(format!("ovt-pending-{}.db", uuid::Uuid::new_v4()));
        let repo = PendingRepo::open(&path).unwrap();
        repo.save(&task("capture")).unwrap();
        repo.transition("capture", PendingStatus::Queued, 20)
            .unwrap();
        repo.transition("capture", PendingStatus::Transcribing, 30)
            .unwrap();
        assert_eq!(repo.recover_interrupted(40).unwrap(), 1);
        let recovered = repo.get("capture").unwrap().unwrap();
        assert_eq!(recovered.status, PendingStatus::Interrupted);
        assert!(!recovered.status.allows_automatic_delivery());

        repo.transition("capture", PendingStatus::Queued, 50)
            .unwrap();
        repo.transition("capture", PendingStatus::Transcribing, 60)
            .unwrap();
        repo.transition("capture", PendingStatus::NeedsDelivery, 70)
            .unwrap();
        repo.transition("capture", PendingStatus::Delivering, 80)
            .unwrap();
        assert_eq!(repo.recover_interrupted(90).unwrap(), 1);
        assert_eq!(
            repo.get("capture").unwrap().unwrap().status,
            PendingStatus::NeedsReview
        );
        drop(repo);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn corrupt_row_is_reported_and_does_not_break_other_rows() {
        let path = std::env::temp_dir().join(format!("ovt-pending-{}.db", uuid::Uuid::new_v4()));
        let repo = PendingRepo::open(&path).unwrap();
        repo.save(&task("good")).unwrap();
        repo.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO pending_tasks VALUES ('bad', 's', 'failed', 0, 0, 'dictate', 'x', '{bad')",
                [],
            )
            .unwrap();
        assert!(repo.list().is_err());
        assert!(repo.get("good").is_ok());
        assert!(repo.delete("bad").is_ok());
        drop(repo);
        let _ = std::fs::remove_file(path);
    }
}
