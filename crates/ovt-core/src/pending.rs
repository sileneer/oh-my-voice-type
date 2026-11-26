//! Durable pending-task model and state machine. Persistence is owned by the
//! desktop host; this module deliberately contains no database or Tauri types.

use crate::config::SessionMode;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const DEFAULT_PENDING_RETENTION_DAYS: u32 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingStatus {
    #[default]
    Capturing,
    Queued,
    Transcribing,
    Rewriting,
    NeedsDelivery,
    Delivering,
    Completed,
    Failed,
    Interrupted,
    NeedsReview,
}

impl PendingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capturing => "capturing",
            Self::Queued => "queued",
            Self::Transcribing => "transcribing",
            Self::Rewriting => "rewriting",
            Self::NeedsDelivery => "needs_delivery",
            Self::Delivering => "delivering",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::NeedsReview => "needs_review",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "capturing" => Self::Capturing,
            "queued" => Self::Queued,
            "transcribing" => Self::Transcribing,
            "rewriting" => Self::Rewriting,
            "needs_delivery" => Self::NeedsDelivery,
            "delivering" => Self::Delivering,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "interrupted" => Self::Interrupted,
            "needs_review" => Self::NeedsReview,
            _ => return None,
        })
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use PendingStatus::*;
        matches!(
            (self, next),
            (Capturing, Queued)
                | (Capturing, Failed)
                | (Queued, Transcribing)
                | (Queued, Failed)
                | (Queued, Interrupted)
                | (Transcribing, Rewriting)
                | (Transcribing, NeedsDelivery)
                | (Transcribing, Failed)
                | (Transcribing, Interrupted)
                | (Rewriting, NeedsDelivery)
                | (Rewriting, Failed)
                | (Rewriting, Interrupted)
                | (NeedsDelivery, Delivering)
                | (NeedsDelivery, Rewriting)
                | (NeedsDelivery, Failed)
                | (NeedsDelivery, Interrupted)
                | (Delivering, Completed)
                | (Delivering, NeedsReview)
                | (Delivering, Failed)
                | (Failed, Queued)
                | (Interrupted, Queued)
                | (NeedsReview, Queued)
                | (NeedsReview, Rewriting)
        )
    }

    /// Recovery must never silently insert text into a target that may have
    /// changed while the process was down.
    pub fn allows_automatic_delivery(self) -> bool {
        !matches!(self, Self::Interrupted | Self::NeedsReview)
    }

    pub fn allows_retry(self) -> bool {
        matches!(
            self,
            Self::Failed
                | Self::Interrupted
                | Self::Queued
                | Self::NeedsDelivery
                | Self::NeedsReview
        )
    }

    pub fn is_open(self) -> bool {
        !matches!(self, Self::Completed)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PendingTask {
    pub id: String,
    pub session_id: String,
    pub mode: SessionMode,
    pub target_language: Option<String>,
    pub status: PendingStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub audio_path: String,
    pub sample_rate: u32,
    pub frame_count: u64,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub target_id: Option<String>,
    pub selection_revision: Option<String>,
    pub selection_hash: Option<String>,
    pub selection: Option<String>,
    pub secure_field: bool,
    pub raw_text: String,
    pub final_text: String,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub attempts: u32,
}

impl PendingTask {
    pub fn new(id: String, session_id: String, mode: SessionMode) -> Self {
        Self {
            id,
            session_id,
            mode,
            audio_path: String::new(),
            sample_rate: 16_000,
            ..Self::default()
        }
    }

    pub fn transition(&mut self, next: PendingStatus, now_ms: u64) -> bool {
        if self.status == next {
            self.updated_at_ms = now_ms;
            return true;
        }
        if !self.status.can_transition_to(next) {
            return false;
        }
        self.status = next;
        self.updated_at_ms = now_ms;
        if matches!(next, PendingStatus::Failed | PendingStatus::Interrupted) {
            self.error = Some(next.as_str().to_string());
        }
        true
    }

    pub fn recovery_status(&self) -> PendingStatus {
        match self.status {
            PendingStatus::Delivering => PendingStatus::NeedsReview,
            status if status.is_open() => PendingStatus::Interrupted,
            status => status,
        }
    }

    pub fn expired(&self, now_ms: u64, retention_days: u32) -> bool {
        if !matches!(
            self.status,
            PendingStatus::Failed | PendingStatus::Interrupted | PendingStatus::NeedsReview
        ) {
            return false;
        }
        let cutoff = now_ms.saturating_sub(
            u64::from(retention_days.clamp(1, 30)) * Duration::from_secs(86_400).as_millis() as u64,
        );
        self.updated_at_ms < cutoff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_lifecycle_and_review_path() {
        let mut task = PendingTask::new("p".into(), "s".into(), SessionMode::Dictate);
        for status in [
            PendingStatus::Queued,
            PendingStatus::Transcribing,
            PendingStatus::Rewriting,
            PendingStatus::NeedsDelivery,
            PendingStatus::Delivering,
            PendingStatus::Completed,
        ] {
            assert!(task.transition(status, 1), "could not enter {status:?}");
        }
        assert!(task.status.allows_automatic_delivery());

        let mut task = PendingTask::new("p".into(), "s".into(), SessionMode::Dictate);
        task.transition(PendingStatus::Queued, 1);
        task.transition(PendingStatus::Transcribing, 2);
        task.transition(PendingStatus::NeedsDelivery, 3);
        task.transition(PendingStatus::Delivering, 4);
        assert!(task.transition(PendingStatus::NeedsReview, 5));
        assert!(!task.status.allows_automatic_delivery());
        assert!(task.status.allows_retry());
        assert!(task.transition(PendingStatus::Rewriting, 6));
    }

    #[test]
    fn invalid_skips_are_rejected() {
        let mut task = PendingTask::new("p".into(), "s".into(), SessionMode::Translate);
        assert!(!task.transition(PendingStatus::Completed, 1));
        assert!(!task.transition(PendingStatus::Delivering, 1));
        assert_eq!(task.status, PendingStatus::Capturing);
    }

    #[test]
    fn crash_recovery_is_conservative_and_expiry_is_bounded() {
        let mut task = PendingTask::new("p".into(), "s".into(), SessionMode::Dictate);
        task.transition(PendingStatus::Queued, 100);
        assert_eq!(task.clone().recovery_status(), PendingStatus::Interrupted);
        task.transition(PendingStatus::Failed, 200);
        let week_ms = 7 * 86_400_000;
        assert!(!task.expired(200 + week_ms, 7));
        assert!(task.expired(201 + week_ms, 7));
        assert!(task.expired(u64::MAX, 0));
        assert!(task.expired(u64::MAX, 31));
    }
}
