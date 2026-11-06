// Yard — offline batch file compression
// Copyright (C) 2026 Yard Contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub(crate) const PROGRESS_EVENT: &str = "file:progress";

pub(crate) fn progress_payload(file_id: &str, percent: u32) -> serde_json::Value {
    serde_json::json!({ "fileId": file_id, "percent": percent })
}

#[derive(Clone)]
pub struct ProgressSender {
    emit: Arc<dyn Fn(u32) + Send + Sync>,
}

impl ProgressSender {
    pub fn new<R: tauri::Runtime>(app: AppHandle<R>, file_id: String) -> Self {
        Self::new_with_emitter(Arc::new(move |percent| {
            // Event-emit failures are non-actionable: serialization is
            // infallible for the fixed two-field payload; a missing listener
            // is normal in early lifecycle.
            let _ = app.emit(PROGRESS_EVENT, progress_payload(&file_id, percent));
        }))
    }

    pub(crate) fn new_with_emitter(emitter: Arc<dyn Fn(u32) + Send + Sync>) -> Self {
        ProgressSender { emit: emitter }
    }

    #[cfg(test)]
    pub fn test_stub() -> Self {
        Self::new_with_emitter(Arc::new(|_| {}))
    }

    /// Test double that records every emitted percent. Shared by processor
    /// tests asserting monotonicity, slice coverage, and terminal progress.
    #[cfg(test)]
    pub fn test_collector() -> (Self, Arc<std::sync::Mutex<Vec<u32>>>) {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        (
            Self::new_with_emitter(Arc::new(move |percent| {
                sink.lock().unwrap().push(percent);
            })),
            seen,
        )
    }

    pub fn send_progress(&self, percent: u32) {
        (self.emit)(percent.clamp(0, 100));
    }

    /// Maps attempt-local 0..=100 onto a single pre-delivery ceiling and
    /// suppresses any value below the high-water mark, so multiple attempts
    /// can share one progress bar without double scaling or regressions.
    pub(crate) fn scaled(&self, ceiling: u32) -> Self {
        let inner = self.clone();
        let high_water = Arc::new(std::sync::Mutex::new(0u32));
        Self::new_with_emitter(Arc::new(move |percent| {
            let mapped = (percent.min(100) * ceiling) / 100;
            let mut sent = high_water.lock().unwrap();
            if mapped > *sent {
                *sent = mapped;
                drop(sent);
                inner.send_progress(mapped);
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_progress_preserves_in_range_values() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let sender = ProgressSender::new_with_emitter(Arc::new(move |percent| {
            sink.lock().unwrap().push(percent);
        }));
        sender.send_progress(1);
        sender.send_progress(50);
        sender.send_progress(99);
        assert_eq!(*seen.lock().unwrap(), vec![1, 50, 99]);
    }

    #[test]
    fn send_progress_clamps_out_of_range_values() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let sender = ProgressSender::new_with_emitter(Arc::new(move |percent| {
            sink.lock().unwrap().push(percent);
        }));
        sender.send_progress(0);
        sender.send_progress(45);
        sender.send_progress(100);
        sender.send_progress(101);
        sender.send_progress(150);
        sender.send_progress(7);
        assert_eq!(*seen.lock().unwrap(), vec![0, 45, 100, 100, 100, 7]);
    }

    #[test]
    fn progress_payload_uses_camel_case_file_id_key() {
        let payload = progress_payload("abc-123", 72);
        assert_eq!(
            payload["fileId"], "abc-123",
            "payload key must be camelCase fileId"
        );
        assert!(
            payload.get("file_id").is_none(),
            "snake_case key would break the TS hook"
        );
        assert_eq!(payload["percent"], 72);
    }

    #[test]
    fn progress_event_topic_is_pinned() {
        assert_eq!(PROGRESS_EVENT, "file:progress");
    }
}
