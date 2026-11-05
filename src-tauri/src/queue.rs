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

use crate::commands::FileListState;
use crate::engine::{CompressionError, CompressionResult, ProgressSender};
use crate::history::{
    Counts, FileResultRecord, HistoryRecordRef, HistoryStore, SessionEndRecord, SessionStartRecord,
};
use crate::models::{FileEntry, FileStatus};
use crate::processors::ProcessorRegistry;
use crate::scheduler;
use crate::sidecar;
use crate::temp;
use std::collections::{HashMap, HashSet};
use std::sync::{LockResult, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn poison_recover<T>(lock: LockResult<MutexGuard<'_, T>>) -> MutexGuard<'_, T> {
    lock.unwrap_or_else(|e| {
        eprintln!("warn: queue state mutex was poisoned, recovering: {e}");
        e.into_inner()
    })
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(msg) = payload.downcast_ref::<&'static str>() {
        format!("job panicked: {msg}")
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        format!("job panicked: {msg}")
    } else {
        "job panicked".to_string()
    }
}

fn run_processor_catch_panic(
    f: impl FnOnce() -> Result<CompressionResult, CompressionError>,
) -> Result<CompressionResult, CompressionError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|payload| Err(CompressionError::OutputFileError(panic_payload_message(&payload))))
}

/// HEIC/HEIF input → .jpg output and MOV input → .mp4 output (spec §3),
/// case-insensitive. Resolving the real output extension here is what makes
/// collision detection apply before any processor touches the filename.
pub(crate) fn output_file_name(name: &str) -> String {
    let lower = name.to_lowercase();
    let output_ext: Option<&str> = if lower.ends_with(".heic") || lower.ends_with(".heif") {
        Some("jpg")
    } else if lower.ends_with(".mov") {
        Some("mp4")
    } else {
        None
    };
    match output_ext {
        Some(ext) => name.rsplit_once('.').map_or_else(
            || format!("{name}.{ext}"),
            |(stem, _)| format!("{stem}.{ext}"),
        ),
        None => name.to_string(),
    }
}

/// Map a successful CompressionResult to its final FileStatus.
pub(crate) fn decide_result_status(result: &CompressionResult) -> FileStatus {
    if !result.met_target {
        FileStatus::BestEffort
    } else if result.is_already_under_target() {
        FileStatus::AlreadyUnderTarget
    } else {
        FileStatus::Done
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchMessage {
    batch_id: String,
    file_ids: Vec<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FileErrorPayload<'a> {
    file_id: &'a str,
    error_message: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FileCompletePayload<'a> {
    file_id: &'a str,
    output_path: String,
    original_size: u64,
    compressed_size: u64,
    status: FileStatus,
    met_target: bool,
    quality_used: Option<u32>,
    strategy_used: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FileCancelledPayload<'a> {
    file_id: &'a str,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchCompletePayload<'a> {
    batch_id: &'a str,
    total_saved_bytes: u64,
    cancelled: bool,
}

/// Managed state shared by commands and the worker task.
/// Cancellation goes through this struct directly — NOT through the
/// mpsc channel, whose receiver is busy inside process_batch.
/// Runtime-generic so tests can drive it on tauri's MockRuntime.
pub struct QueueState<R: Runtime = tauri::Wry> {
    token_map: Mutex<HashMap<String, CancellationToken>>,
    batch_tokens: Mutex<HashMap<String, CancellationToken>>,
    history_records: Mutex<HashMap<String, Vec<FileResultRecord>>>,
    history_error_sent: Mutex<HashSet<String>>,
    sender: mpsc::UnboundedSender<BatchMessage>,
    #[cfg(test)]
    test_estimate: Mutex<Option<fn(crate::models::Category, u64, Option<u32>) -> u64>>,
    #[cfg(test)]
    test_available_space: Mutex<Option<fn(&std::path::Path) -> Option<u64>>>,
    _runtime: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> QueueState<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel::<BatchMessage>();
        let worker_app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(msg) = receiver.recv().await {
                Self::process_batch(&worker_app, msg.batch_id, msg.file_ids).await;
            }
        });
        QueueState {
            token_map: Mutex::new(HashMap::new()),
            batch_tokens: Mutex::new(HashMap::new()),
            history_records: Mutex::new(HashMap::new()),
            history_error_sent: Mutex::new(HashSet::new()),
            sender,
            #[cfg(test)]
            test_estimate: Mutex::new(None),
            #[cfg(test)]
            test_available_space: Mutex::new(None),
            _runtime: std::marker::PhantomData,
        }
    }

    /// Called by compress_all: registers the batch token then queues work.
    pub fn start_batch(&self, app: &AppHandle<R>, batch_id: String, file_ids: Vec<String>) {
        let token = CancellationToken::new();
        poison_recover(self.batch_tokens.lock()).insert(batch_id.clone(), token);

        if self
            .sender
            .send(BatchMessage {
                batch_id: batch_id.clone(),
                file_ids,
            })
            .is_err()
        {
            eprintln!("warn: job worker unavailable, batch dropped: {batch_id}");
            let _ = app.emit(
                "batch:error",
                serde_json::json!({ "batchId": batch_id, "errorMessage": "Job worker unavailable" }),
            );
            poison_recover(self.batch_tokens.lock()).remove(&batch_id);
        }
    }

    pub fn cancel_file(&self, file_id: &str) {
        if let Some(token) = poison_recover(self.token_map.lock()).get(file_id) {
            token.cancel();
        }
    }

    pub fn cancel_all(&self) {
        for token in poison_recover(self.batch_tokens.lock()).values() {
            token.cancel();
        }
        for token in poison_recover(self.token_map.lock()).values() {
            token.cancel();
        }
    }

    fn batch_cancelled(&self, batch_id: &str) -> bool {
        poison_recover(self.batch_tokens.lock())
            .get(batch_id)
            .is_some_and(|t| t.is_cancelled())
    }

    fn estimate_entry(&self, entry: &FileEntry) -> u64 {
        #[cfg(test)]
        {
            self.estimate_for(entry.category, entry.original_size, None)
        }
        #[cfg(not(test))]
        {
            crate::memory::estimate_job(entry.category, entry.original_size, None)
        }
    }

    fn available_space_for(&self, path: &std::path::Path) -> Option<u64> {
        #[cfg(test)]
        {
            if let Some(stub) = *poison_recover(self.test_available_space.lock()) {
                return stub(path);
            }
        }
        crate::memory::available_space(path)
    }

    fn insert_token(&self, file_id: &str, token: CancellationToken) {
        poison_recover(self.token_map.lock()).insert(file_id.to_string(), token);
    }

    fn remove_token(&self, file_id: &str) {
        poison_recover(self.token_map.lock()).remove(file_id);
    }

    #[cfg(test)]
    fn batch_token_present(&self, batch_id: &str) -> bool {
        poison_recover(self.batch_tokens.lock()).contains_key(batch_id)
    }

    #[cfg(test)]
    fn history_records_present(&self, batch_id: &str) -> bool {
        poison_recover(self.history_records.lock()).contains_key(batch_id)
    }

    #[cfg(test)]
    fn history_error_pending(&self, batch_id: &str) -> bool {
        poison_recover(self.history_error_sent.lock()).contains(batch_id)
    }

    #[cfg(test)]
    fn set_test_estimate(&self, estimate: fn(crate::models::Category, u64, Option<u32>) -> u64) {
        *poison_recover(self.test_estimate.lock()) = Some(estimate);
    }

    #[cfg(test)]
    fn set_test_available_space(&self, available: fn(&std::path::Path) -> Option<u64>) {
        *poison_recover(self.test_available_space.lock()) = Some(available);
    }

    #[cfg(test)]
    fn estimate_for(&self, category: crate::models::Category, input_size: u64, width: Option<u32>) -> u64 {
        poison_recover(self.test_estimate.lock())
            .map(|f| f(category, input_size, width))
            .unwrap_or_else(|| crate::memory::estimate_job(category, input_size, width))
    }

    #[cfg(test)]
    fn seed_history_state(&self, batch_id: &str) {
        let record = FileResultRecord {
            session_id: batch_id.to_string(),
            file_id: "seeded".to_string(),
            completed_at: 0,
            name: "seeded".to_string(),
            path: "/seeded".to_string(),
            output_path: None,
            category: "image".to_string(),
            target_bytes: 1,
            original_size: 1,
            compressed_size: None,
            status: "cancelled".to_string(),
            quality_used: None,
            strategy_used: None,
            error: None,
            duration_ms: 0,
        };
        poison_recover(self.history_records.lock())
            .entry(batch_id.to_string())
            .or_default()
            .push(record);
        poison_recover(self.history_error_sent.lock()).insert(batch_id.to_string());
    }

    fn record_file(
        &self,
        app: &AppHandle<R>,
        batch_id: &str,
        entry: &FileEntry,
        status: FileStatus,
        output_path: Option<String>,
        compressed_size: Option<u64>,
        quality_used: Option<u32>,
        strategy_used: Option<String>,
        error: Option<String>,
        duration_ms: u64,
    ) {
        let record = FileResultRecord {
            session_id: batch_id.to_string(),
            file_id: entry.id.clone(),
            completed_at: now_ms(),
            name: entry.name.clone(),
            path: entry.path.clone(),
            output_path,
            category: category_key(&entry.category).to_string(),
            target_bytes: entry.target_bytes,
            original_size: entry.original_size,
            compressed_size,
            status: status_key(&status).to_string(),
            quality_used,
            strategy_used,
            error,
            duration_ms,
        };
        poison_recover(self.history_records.lock())
            .entry(batch_id.to_string())
            .or_default()
            .push(record.clone());
        let _ = append_history(app, HistoryRecordRef::File(&record));
    }

    fn record_cancelled(
        &self,
        app: &AppHandle<R>,
        batch_id: &str,
        entry: &FileEntry,
        duration_ms: u64,
    ) {
        self.record_file(
            app,
            batch_id,
            entry,
            FileStatus::Cancelled,
            None,
            None,
            None,
            None,
            None,
            duration_ms,
        );
    }

    async fn process_batch(app: &AppHandle<R>, batch_id: String, file_ids: Vec<String>) {
        let queue = app.state::<QueueState<R>>();
        let state = app.state::<FileListState>();

        let settings = crate::commands::load_settings(app);
        let output_folder = settings.output_folder.clone();

        if let Err(e) = ensure_output_folder_writable(&output_folder) {
            Self::fail_preflight(app, &queue, &batch_id, e);
            return;
        }

        let (estimates, largest_output, largest_estimate) = {
            let list = poison_recover(state.0.lock());
            let mut estimates = HashMap::new();
            let mut largest_output = 0u64;
            let mut largest_estimate = 0u64;
            for file_id in &file_ids {
                let Some(entry) = list
                    .iter()
                    .find(|e| &e.id == file_id && e.status == FileStatus::Pending)
                else {
                    continue;
                };
                let est = queue.estimate_entry(entry);
                estimates.insert(file_id.clone(), est);
                largest_output = largest_output.max(entry.original_size);
                largest_estimate = largest_estimate.max(est);
            }
            (estimates, largest_output, largest_estimate)
        };

        let output_need = largest_output.saturating_mul(11) / 10;
        if let Err(e) = crate::memory::check_disk_headroom(
            output_need,
            queue.available_space_for(std::path::Path::new(&output_folder)),
        ) {
            Self::fail_preflight(app, &queue, &batch_id, e);
            return;
        }

        let temp_base = app
            .path()
            .temp_dir()
            .map(|p| p.join("yard"))
            .unwrap_or_else(|_| std::env::temp_dir().join("yard"));
        let temp_need = largest_estimate / 2;
        if let Err(e) = crate::memory::check_disk_headroom(
            temp_need,
            queue.available_space_for(&temp_base),
        ) {
            Self::fail_preflight(app, &queue, &batch_id, e);
            return;
        }

        let started = SessionStartRecord {
            session_id: batch_id.clone(),
            started_at: now_ms(),
            file_count: file_ids.len(),
            output_folder: output_folder.clone(),
        };
        let _ = append_history(app, HistoryRecordRef::SessionStart(&started));

        let budget = crate::memory::derive_budget(&settings.performance);

        let reserved = std::sync::Arc::new(crate::output::ReservedNames::new());
        let (lanes, placed) = group_lanes(&state, &file_ids, |entry| {
            estimates.get(&entry.id).copied().unwrap_or_else(|| queue.estimate_entry(entry))
        });

        let unplaced_cancelled: Vec<FileEntry> = {
            let list = poison_recover(state.0.lock());
            file_ids
                .iter()
                .filter(|file_id| !placed.contains(*file_id))
                .filter_map(|file_id| {
                    list.iter()
                        .find(|e| &e.id == file_id && e.status == FileStatus::Cancelled)
                        .cloned()
                })
                .collect()
        };
        for entry in unplaced_cancelled {
            queue.record_cancelled(app, &batch_id, &entry, 0);
        }

        let batch_app = app.clone();
        let stop_batch_id = batch_id.clone();
        let stop_flag = move || {
            let q = batch_app.state::<QueueState<R>>();
            q.batch_cancelled(&stop_batch_id)
        };

        let run_app = app.clone();
        let run_batch_id = batch_id.clone();
        let run_folder = output_folder.clone();
        let run_settings = settings.clone();
        let run_reserved = reserved.clone();
        let used = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let emit_app = app.clone();
        let emit_used = used.clone();
        let emit_batch_id = batch_id.clone();
        let emit_handle = tauri::async_runtime::spawn(async move {
            let emit_queue = emit_app.state::<QueueState<R>>();
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
            loop {
                interval.tick().await;
                if emit_queue.batch_cancelled(&emit_batch_id) {
                    break;
                }
                let _ = emit_app.emit(
                    "memory:usage",
                    serde_json::json!({
                        "budgetBytes": budget,
                        "usedBytes": emit_used.load(std::sync::atomic::Ordering::SeqCst),
                        "processRssBytes": crate::memory::process_rss(),
                    }),
                );
            }
        });

        let skip_app = app.clone();
        let skip_batch_id = batch_id.clone();
        let skip_state = app.state::<FileListState>();
        let total_saved = scheduler::run_budgeted_lanes(
            lanes,
            budget,
            stop_flag,
            move |job| {
                let app = run_app.clone();
                let batch_id = run_batch_id.clone();
                let output_folder = run_folder.clone();
                let settings = run_settings.clone();
                let reserved = run_reserved.clone();
                let used = used.clone();
                async move {
                    used.fetch_add(job.estimate, std::sync::atomic::Ordering::SeqCst);
                    let result = Self::process_single_job(
                        app,
                        batch_id,
                        job.file_id,
                        output_folder,
                        settings,
                        &reserved,
                    )
                    .await;
                    used.fetch_sub(job.estimate, std::sync::atomic::Ordering::SeqCst);
                    result
                }
            },
            move |job| {
                emit_file_cancelled(&skip_app, &job.file_id);
                let entry = {
                    let mut list = poison_recover(skip_state.0.lock());
                    cancel_skip_entry(&mut list, &job.file_id)
                };
                if let Some(entry) = entry {
                    let queue = skip_app.state::<QueueState<R>>();
                    queue.record_cancelled(&skip_app, &skip_batch_id, &entry, 0);
                }
            },
            |job| job.estimate,
        )
        .await;
        emit_handle.abort();
        let _ = emit_handle.await;
        Self::finish_batch(app, &batch_id, total_saved);
    }

    fn fail_preflight(app: &AppHandle<R>, queue: &tauri::State<'_, QueueState<R>>, batch_id: &str, error: String) {
        poison_recover(queue.history_records.lock()).remove(batch_id);
        poison_recover(queue.history_error_sent.lock()).remove(batch_id);
        poison_recover(queue.batch_tokens.lock()).remove(batch_id);
        let _ = app.emit(
            "batch:error",
            serde_json::json!({ "batchId": batch_id, "errorMessage": error }),
        );
    }

    async fn process_single_job(
        app: AppHandle<R>,
        batch_id: String,
        file_id: String,
        output_folder: String,
        settings: crate::settings::Settings,
        reserved: &crate::output::ReservedNames,
    ) -> u64 {
        let queue = app.state::<QueueState<R>>();
        let registry = ProcessorRegistry::new();
        let state = app.state::<FileListState>();
        let mut total_saved: u64 = 0;
        let started = std::time::Instant::now();
        let elapsed_ms = || (started.elapsed().as_millis() as u64).max(1);

        if queue.batch_cancelled(&batch_id) {
            let entry = {
                let mut list = poison_recover(state.0.lock());
                cancel_skip_entry(&mut list, &file_id)
            };
            if let Some(entry) = entry {
                queue.record_cancelled(&app, &batch_id, &entry, 0);
            }
            emit_file_cancelled(&app, &file_id);
            return total_saved;
        }

        let entry: Option<FileEntry> = {
            let list = poison_recover(state.0.lock());
            list.iter().find(|e| e.id == file_id).cloned()
        };
        let Some(entry) = entry else { return total_saved };
        if entry.status != FileStatus::Pending {
            if entry.status == FileStatus::Cancelled {
                queue.record_cancelled(&app, &batch_id, &entry, 0);
            }
            return total_saved; // cancelled/removed/done — skip
        }

        // Insert token BEFORE flipping status → cancel_file never misses.
        // Re-check status after inserting: a concurrent cancel_file may
        // have marked the entry Cancelled between the clone and now.
        let cancel_token = CancellationToken::new();
        queue.insert_token(&file_id, cancel_token.clone());

        let flipped = {
            let mut list = poison_recover(state.0.lock());
            match list.iter_mut().find(|e| e.id == file_id) {
                Some(e) if e.status == FileStatus::Pending => {
                    e.status = FileStatus::Compressing;
                    true
                }
                _ => false,
            }
        };
        if !flipped {
            queue.remove_token(&file_id);
            let cancelled = {
                let mut list = poison_recover(state.0.lock());
                cancel_skip_entry(&mut list, &file_id)
            };
            if let Some(cancelled) = cancelled {
                queue.record_cancelled(&app, &batch_id, &cancelled, 0);
            }
            return total_saved;
        }

        let progress = ProgressSender::new(app.clone(), file_id.clone());

        let result = match registry.get_processor(entry.category) {
            None => {
                // No processor registered for this category (Other —
                // unsupported formats): skip as Unsupported — NOT
                // Failed (spec §3/§6)
                {
                    let mut list = poison_recover(state.0.lock());
                    if let Some(e) = list.iter_mut().find(|e| e.id == file_id) {
                        e.status = FileStatus::Unsupported;
                    }
                }
                queue.record_file(
                    &app,
                    &batch_id,
                    &entry,
                    FileStatus::Unsupported,
                    None,
                    None,
                    None,
                    None,
                    None,
                    elapsed_ms(),
                );
                let _ = app.emit(
                    "file:complete",
                    FileCompletePayload {
                        file_id: &file_id,
                        output_path: String::new(),
                        original_size: entry.original_size,
                        compressed_size: 0,
                        status: FileStatus::Unsupported,
                        met_target: false,
                        quality_used: None,
                        strategy_used: None,
                    },
                );
                queue.remove_token(&file_id);
                return total_saved;
            }
            Some(processor) => {
                let temp_dir = match temp::create_temp_dir(&app) {
                    Ok(d) => d,
                    Err(e) => {
                        let error_msg = e.clone();
                        {
                            let mut list = poison_recover(state.0.lock());
                            if let Some(e2) = list.iter_mut().find(|e2| e2.id == file_id) {
                                e2.status = FileStatus::Failed;
                                e2.error = Some(error_msg.clone());
                            }
                        }
                        queue.record_file(
                            &app,
                            &batch_id,
                            &entry,
                            FileStatus::Failed,
                            None,
                            None,
                            None,
                            None,
                            Some(error_msg.clone()),
                            elapsed_ms(),
                        );
                        let _ = app.emit(
                            "file:error",
                            FileErrorPayload {
                                file_id: &file_id,
                                error_message: error_msg,
                            },
                        );
                        queue.remove_token(&file_id);
                        return total_saved;
                    }
                };

                let output_filename = output_file_name(&entry.name);
                let output_path =
                    crate::output::resolve_output_path_reserved(&output_folder, &output_filename, reserved);

                let run = match sidecar::resolve_all_sidecar_paths() {
                    Ok(sidecar_paths) => {
                        run_processor_catch_panic(|| {
                            processor.compress_to_target(
                                std::path::Path::new(&entry.path),
                                &output_path,
                                entry.target_bytes,
                                &settings.quality_floors,
                                cancel_token,
                                progress,
                                &sidecar_paths,
                                &temp_dir,
                            )
                        })
                    }
                    // resolve_all_sidecar_paths already carries the
                    // "Engine error: …" wording; Unknown only re-labels
                    // pre-formatted text. A portable test trigger is
                    // hard: happy-path exe dirs always resolve, and
                    // simulating a broken current_exe() (no parent dir,
                    // non-UTF-8 cwd) is not feasible from a spawned test
                    // binary — the failure shape is pinned indirectly via
                    // sidecar.rs resolve/resolve_for unit tests.
                    Err(e) => Err(CompressionError::Unknown(e)),
                };

                // Temp cleaned after each job, even on error/cancel (spec §6)
                temp::cleanup_temp_dir(&temp_dir);
                run
            }
        };

        match result {
            Ok(compression_result) => {
                let saved = entry
                    .original_size
                    .saturating_sub(compression_result.compressed_size);
                total_saved += saved;
                let status = decide_result_status(&compression_result);
                let out_path = compression_result.output_path.to_string_lossy().to_string();
                let quality_used = Some(compression_result.quality_used);
                let strategy_used = Some(compression_result.strategy_used.clone());

                {
                    let mut list = poison_recover(state.0.lock());
                    if let Some(e) = list.iter_mut().find(|e| e.id == file_id) {
                        e.status = status;
                        e.compressed_size = Some(compression_result.compressed_size);
                        e.output_path = Some(out_path.clone());
                    }
                }

                queue.record_file(
                    &app,
                    &batch_id,
                    &entry,
                    status,
                    Some(out_path.clone()),
                    Some(compression_result.compressed_size),
                    quality_used,
                    strategy_used.clone(),
                    None,
                    elapsed_ms(),
                );

                let _ = app.emit(
                    "file:complete",
                    FileCompletePayload {
                        file_id: &file_id,
                        output_path: out_path,
                        original_size: entry.original_size,
                        compressed_size: compression_result.compressed_size,
                        status,
                        met_target: compression_result.met_target,
                        quality_used,
                        strategy_used,
                    },
                );
            }
            Err(CompressionError::Cancelled) => {
                {
                    let mut list = poison_recover(state.0.lock());
                    if let Some(e) = list.iter_mut().find(|e| e.id == file_id) {
                        e.status = FileStatus::Cancelled;
                    }
                }
                queue.record_cancelled(&app, &batch_id, &entry, elapsed_ms());
                emit_file_cancelled(&app, &file_id);
            }
            Err(e) => {
                let error_msg = e.to_string();
                {
                    let mut list = poison_recover(state.0.lock());
                    if let Some(e2) = list.iter_mut().find(|e2| e2.id == file_id) {
                        e2.status = FileStatus::Failed;
                        e2.error = Some(error_msg.clone());
                    }
                }
                queue.record_file(
                    &app,
                    &batch_id,
                    &entry,
                    FileStatus::Failed,
                    None,
                    None,
                    None,
                    None,
                    Some(error_msg.clone()),
                    elapsed_ms(),
                );
                let _ = app.emit(
                    "file:error",
                    FileErrorPayload {
                        file_id: &file_id,
                        error_message: error_msg,
                    },
                );
            }
        }

        queue.remove_token(&file_id);
        total_saved
    }

    fn finish_batch(app: &AppHandle<R>, batch_id: &str, total_saved: u64) {
        let queue = app.state::<QueueState<R>>();
        let cancelled = queue.batch_cancelled(batch_id);
        poison_recover(queue.batch_tokens.lock()).remove(batch_id);

        let records = poison_recover(queue.history_records.lock()).remove(batch_id);
        let records = records.unwrap_or_default();
        let counts = history_counts_of(&records);
        let end = SessionEndRecord {
            session_id: batch_id.to_string(),
            ended_at: now_ms(),
            total_saved_bytes: total_saved,
            counts,
            cancelled,
        };
        let _ = append_history(app, HistoryRecordRef::SessionEnd(&end));

        let settings = crate::commands::load_settings(app);
        match app.try_state::<HistoryStore>() {
            Some(store) => {
                if let Err(e) =
                    store.trim(settings.history.effective_retention_days(), now_ms())
                {
                    eprintln!("warn: history trim after batch {batch_id} failed: {e}");
                }
            }
            None => {
                let first =
                    poison_recover(queue.history_error_sent.lock()).insert(batch_id.to_string());
                if first {
                    eprintln!(
                        "warn: history store not managed, skipping history trim for batch {batch_id}"
                    );
                }
            }
        }

        poison_recover(queue.history_error_sent.lock()).remove(batch_id);

        let _ = app.emit(
            "batch:complete",
            BatchCompletePayload {
                batch_id,
                total_saved_bytes: total_saved,
                cancelled,
            },
        );
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn status_key(status: &FileStatus) -> &'static str {
    match status {
        FileStatus::Pending => "pending",
        FileStatus::Compressing => "compressing",
        FileStatus::Done => "done",
        FileStatus::BestEffort => "best_effort",
        FileStatus::Failed => "failed",
        FileStatus::Cancelled => "cancelled",
        FileStatus::Unsupported => "unsupported",
        FileStatus::AlreadyUnderTarget => "already_under_target",
    }
}

fn history_counts_of(files: &[FileResultRecord]) -> Counts {
    let mut counts = Counts::default();
    for file in files {
        match file.status.as_str() {
            "done" => counts.done += 1,
            "best_effort" => counts.best_effort += 1,
            "failed" => counts.failed += 1,
            "cancelled" => counts.cancelled += 1,
            "unsupported" => counts.unsupported += 1,
            "already_under_target" => counts.already_under_target += 1,
            other => {
                eprintln!("warn: history counts ignored unknown status: {other}");
            }
        }
    }
    counts
}

fn append_history<R: tauri::Runtime>(
    app: &AppHandle<R>,
    record: HistoryRecordRef<'_>,
) -> Result<(), String> {
    let Some(store) = app.try_state::<HistoryStore>() else {
        let batch_id = batch_id_of(&record).to_string();
        let queue = app.state::<QueueState<R>>();
        let first = poison_recover(queue.history_error_sent.lock()).insert(batch_id.clone());
        if first {
            eprintln!(
                "warn: history store not managed, skipping {} record for batch {batch_id}",
                record_kind(&record)
            );
        }
        return Ok(());
    };
    let result = match record {
        HistoryRecordRef::SessionStart(rec) => store.append_session_start(rec),
        HistoryRecordRef::File(rec) => store.append_file(rec),
        HistoryRecordRef::SessionEnd(rec) => store.append_session_end(rec),
    };
    if let Err(e) = &result {
        eprintln!("warn: history write failed: {e}");
        let queue = app.state::<QueueState<R>>();
        let first = poison_recover(queue.history_error_sent.lock()).insert(batch_id_of(&record).to_string());
        if first {
            if let Err(emit_err) = app.emit("history:error", serde_json::json!({ "message": e })) {
                eprintln!("warn: failed to emit history:error: {emit_err}");
            }
        }
    }
    result
}

fn batch_id_of<'a>(record: &'a HistoryRecordRef<'_>) -> &'a str {
    match record {
        HistoryRecordRef::SessionStart(rec) => &rec.session_id,
        HistoryRecordRef::File(rec) => &rec.session_id,
        HistoryRecordRef::SessionEnd(rec) => &rec.session_id,
    }
}

fn record_kind(record: &HistoryRecordRef<'_>) -> &'static str {
    match record {
        HistoryRecordRef::SessionStart(_) => "session-start",
        HistoryRecordRef::File(_) => "file",
        HistoryRecordRef::SessionEnd(_) => "session-end",
    }
}

fn cancel_skip_entry(list: &mut Vec<FileEntry>, file_id: &str) -> Option<FileEntry> {
    let entry = list.iter_mut().find(|e| e.id == file_id)?;
    match entry.status {
        FileStatus::Pending => entry.status = FileStatus::Cancelled,
        FileStatus::Cancelled => {}
        _ => return None,
    }
    Some(entry.clone())
}

fn category_key(category: &crate::models::Category) -> &'static str {
    use crate::models::Category;
    match category {
        Category::Image => "image",
        Category::Video => "video",
        Category::Pdf => "pdf",
        Category::Document => "document",
        Category::Other => "other",
    }
}

fn group_lanes(
    state: &tauri::State<'_, FileListState>,
    file_ids: &[String],
    estimate: impl Fn(&FileEntry) -> u64,
) -> (Vec<scheduler::LaneSpec<scheduler::LaneJob>>, HashSet<String>) {
    let list = poison_recover(state.0.lock());
    let mut lanes: Vec<scheduler::LaneSpec<scheduler::LaneJob>> = Vec::new();
    let mut placed: HashSet<String> = HashSet::new();
    for file_id in file_ids {
        let Some(entry) = list
            .iter()
            .find(|e| &e.id == file_id && e.status == FileStatus::Pending)
        else {
            continue;
        };
        placed.insert(file_id.clone());
        let key = category_key(&entry.category).to_string();
        let job = scheduler::LaneJob {
            file_id: file_id.clone(),
            estimate: estimate(entry),
        };
        if let Some(lane) = lanes.iter_mut().find(|l| l.key == key) {
            lane.jobs.push_back(job);
        } else {
            lanes.push(scheduler::LaneSpec {
                key,
                jobs: std::collections::VecDeque::from(vec![job]),
            });
        }
    }
    (lanes, placed)
}

/// Emits the file:cancelled event (used by the worker and the cancel command).
pub(crate) fn emit_file_cancelled<R: tauri::Runtime>(app: &AppHandle<R>, file_id: &str) {
    let _ = app.emit("file:cancelled", FileCancelledPayload { file_id });
}

fn ensure_output_folder_writable(folder: &str) -> Result<(), String> {
    let path = std::path::Path::new(folder);
    if !path.exists() {
        std::fs::create_dir_all(path).map_err(|e| format!("Output folder not writable: {}", e))?;
    }
    let probe = path.join(".write-probe");
    std::fs::write(&probe, b"ok")
        .map(|_| {
            if let Err(e) = std::fs::remove_file(&probe) {
                eprintln!("warn: failed to remove write-probe file: {e}");
            }
        })
        .map_err(|e| format!("Output folder not writable: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{CompressionProcessor, CompressionResult};

    fn result(met: bool, strategy: &str) -> CompressionResult {
        CompressionResult {
            output_path: std::path::PathBuf::from("out"),
            compressed_size: 100,
            quality_used: 75,
            met_target: met,
            strategy_used: strategy.to_string(),
        }
    }

    #[test]
    fn decide_result_status_maps_already_under_target() {
        assert_eq!(
            decide_result_status(&result(true, "already-under-target")),
            FileStatus::AlreadyUnderTarget
        );
    }

    #[test]
    fn decide_result_status_maps_met_target_to_done() {
        assert_eq!(
            decide_result_status(&result(true, "quality=75")),
            FileStatus::Done
        );
    }

    #[test]
    fn decide_result_status_maps_unmet_target_to_best_effort() {
        assert_eq!(
            decide_result_status(&result(false, "quality=30,best-effort")),
            FileStatus::BestEffort
        );
    }

    #[test]
    fn decide_result_status_unmet_target_beats_already_under_strategy() {
        assert_eq!(
            decide_result_status(&result(false, "already-under-target")),
            FileStatus::BestEffort
        );
    }

    #[test]
    fn panic_payload_message_downcasts_common_payloads() {
        assert_eq!(panic_payload_message(&"boom"), "job panicked: boom");
        assert_eq!(
            panic_payload_message(&"boom".to_string()),
            "job panicked: boom"
        );
        let payload: Box<dyn std::any::Any + Send> = Box::new(41u32);
        assert_eq!(panic_payload_message(&payload), "job panicked");
    }

    #[test]
    fn output_file_name_rewrites_heic_to_jpg() {
        assert_eq!(output_file_name("photo.heic"), "photo.jpg");
        assert_eq!(output_file_name("photo.HEIC"), "photo.jpg");
        assert_eq!(output_file_name("photo.heif"), "photo.jpg");
        assert_eq!(output_file_name("photo.HEIF"), "photo.jpg");
    }

    #[test]
    fn output_file_name_rewrites_mov_to_mp4() {
        assert_eq!(output_file_name("clip.mov"), "clip.mp4");
        assert_eq!(output_file_name("clip.MOV"), "clip.mp4");
    }

    #[test]
    fn output_file_name_keeps_non_heic_name() {
        assert_eq!(output_file_name("photo.png"), "photo.png");
        assert_eq!(output_file_name("doc.PDF"), "doc.PDF");
        assert_eq!(output_file_name("clip.mp4"), "clip.mp4");
    }

    #[test]
    fn output_file_name_handles_name_without_extension() {
        assert_eq!(output_file_name("heicfile"), "heicfile");
    }

    #[test]
    fn output_file_name_handles_dot_heic_stem_only_name() {
        assert_eq!(output_file_name(".heic"), ".jpg");
    }

    /// Regression (review round 1): the .mov → .mp4 rewrite used to happen
    /// in VideoProcessor AFTER resolve_output_path had already deduped
    /// "clip.mov", so a pre-existing clip.mp4 was silently clobbered and two
    /// queued files could resolve to the same output. The rewrite happens
    /// here, before collision resolution — the real output name is what gets
    /// deduped.
    #[test]
    fn mov_collision_uses_real_output_name() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("clip.mp4"), b"pre-existing").unwrap();
        std::fs::write(dir.path().join("clip.mov"), b"queued").unwrap();

        let resolved = crate::output::resolve_output_path(
            dir.path().to_str().unwrap(),
            &output_file_name("clip.mov"),
        );
        assert_eq!(
            resolved.file_name().unwrap(),
            "clip-1.mp4",
            "collision detection must apply to the real mp4 name"
        );
        // Neither queued file may be overwritten at resolve time.
        assert_eq!(
            std::fs::read(dir.path().join("clip.mp4")).unwrap(),
            b"pre-existing".to_vec()
        );
        assert_eq!(
            std::fs::read(dir.path().join("clip.mov")).unwrap(),
            b"queued".to_vec()
        );
    }

    #[test]
    fn mov_name_without_collision_resolves_unchanged() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let resolved = crate::output::resolve_output_path(
            dir.path().to_str().unwrap(),
            &output_file_name("clip.mov"),
        );
        assert_eq!(resolved.file_name().unwrap(), "clip.mp4");
    }

    #[test]
    fn preflight_skips_when_folder_already_writable() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        assert!(
            ensure_output_folder_writable(dir.path().to_str().unwrap()).is_ok(),
            "an existing writable folder must pass the preflight unchanged"
        );
        assert!(
            !dir.path().join(".write-probe").exists(),
            "no probe file may be left behind"
        );
    }

    #[test]
    fn write_test_file_removed_after_preflight() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let folder = dir.path().to_str().unwrap().to_string();
        assert!(ensure_output_folder_writable(&folder).is_ok());
        assert!(!dir.path().join(".write-probe").exists());

        #[cfg(unix)]
        {
            // Running as root (UID 0) bypasses DAC permission bits, so a
            // 0o500 folder would still accept the probe and this assertion
            // would fail deterministically in root CI/container runs. Skip
            // the read-only-dir branch there; the success-path assertions
            // above still run everywhere.
            let is_root = unsafe { libc::geteuid() } == 0;
            if !is_root {
                use std::os::unix::fs::PermissionsExt;
                let read_only = dir.path().join("read-only");
                std::fs::create_dir(&read_only).unwrap();
                let mode = |m: u32| {
                    let mut perms = std::fs::metadata(&read_only).unwrap().permissions();
                    perms.set_mode(m);
                    perms
                };
                std::fs::set_permissions(&read_only, mode(0o500)).unwrap();

                let err = ensure_output_folder_writable(read_only.to_str().unwrap());
                assert!(
                    err.is_err(),
                    "a read-only folder must fail the writability probe"
                );
                assert!(
                    !read_only.join(".write-probe").exists(),
                    "failed probe must also be cleaned up"
                );

                std::fs::set_permissions(&read_only, mode(0o755)).unwrap();
            }
        }
    }

    #[test]
    fn queue_batch_creates_missing_output_folder() {
        use tempfile::tempdir;

        let root = tempdir().unwrap();
        let missing = root.path().join("fresh-folder");
        assert!(ensure_output_folder_writable(missing.to_str().unwrap()).is_ok());
        assert!(
            missing.is_dir(),
            "a folder deleted between runs must be recreated by preflight"
        );
        assert!(!missing.join(".write-probe").exists());
    }

    #[test]
    fn queue_batch_create_dir_fails_emits_cannot_create_error() {
        use tempfile::tempdir;

        let root = tempdir().unwrap();
        let blocker = root.path().join("blocker.txt");
        std::fs::write(&blocker, b"regular file").unwrap();
        let blocked = blocker.join("child");

        let err = ensure_output_folder_writable(blocked.to_str().unwrap());
        let msg = err.expect_err("create_dir_all under a regular file must fail");
        assert_eq!(
            msg,
            format!(
                "Output folder not writable: {}",
                std::fs::create_dir_all(&blocked).unwrap_err()
            ),
            "shipped user-facing copy is 'Output folder not writable: <io error>' (spec §6) — plan snippet's 'Cannot create output folder' was substituted; do not regress wording"
        );
        assert!(
            !blocker.parent().unwrap().join(".write-probe").exists(),
            "failed preflight must not leave a probe in the parent"
        );
    }

    #[test]
    fn queue_batch_unwritable_folder_emits_batch_error_and_processes_nothing() {
        use tauri::{Listener, Manager};
        use tempfile::TempDir;

        let root = TempDir::new().unwrap();
        let blocker = root.path().join("blocker.txt");
        std::fs::write(&blocker, b"regular file").unwrap();
        let output_folder = blocker.join("unwritable-child");

        let settings_store = root.path().join("settings.json");
        std::fs::write(
            &settings_store,
            serde_json::json!({ "settings": { "outputFolder": output_folder.to_str().unwrap() } })
                .to_string(),
        )
        .unwrap();

        let mut ctx = tauri::test::mock_context(tauri::test::noop_assets());
        ctx.config_mut().app.app_directories_override =
            Some(tauri::utils::config::AppDirectoriesOverride::Root(
                root.path().to_path_buf(),
            ));
        ctx.config_mut().identifier = "com.app.test".into();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .manage(FileListState(std::sync::Mutex::new(Vec::new())))
            .build(ctx)
            .unwrap();
        app.manage(QueueState::new(app.handle().clone()));

        let (tx, rx) = std::sync::mpsc::channel::<(String, serde_json::Value)>();
        let event_sink = |topic: &'static str| {
            let tx = tx.clone();
            move |event: tauri::Event| {
                let _ = tx.send((
                    topic.to_string(),
                    serde_json::from_str(event.payload()).unwrap(),
                ));
            }
        };
        let _ = app.listen("batch:error", event_sink("batch:error"));
        let _ = app.listen("file:error", event_sink("file:error"));
        let _ = app.listen("file:complete", event_sink("file:complete"));

        let entry = crate::models::FileEntry::new(
            "does-not-matter.jpg",
            crate::detection::default_target_bytes(crate::models::Category::Image),
        );
        {
            let state = app.state::<FileListState>();
            let mut list = state.0.lock().unwrap();
            list.push(entry.clone());
        }
        let batch_id = "test-batch-id".to_string();

        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(
            queue.inner(),
            &app.handle(),
            batch_id.clone(),
            vec![entry.id.clone()],
        );

        for _ in 0..250 {
            let worker_idle = !queue.batch_token_present(&batch_id);
            if worker_idle {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let (topic, payload) = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("preflight failure must emit batch:error");
        assert_eq!(topic, "batch:error");
        assert_eq!(payload["batchId"], batch_id);
        assert!(
            payload["errorMessage"]
                .as_str()
                .unwrap()
                .starts_with("Output folder not writable:"),
            "errorMessage must carry the shipped preflight copy: {payload}"
        );

        let state = app.state::<FileListState>();
        let list = state.0.lock().unwrap();
        assert!(
            list.iter().all(|e| e.status == crate::models::FileStatus::Pending),
            "preflight must run before the first job — file statuses: {:?}",
            list.iter().map(|e| e.status).collect::<Vec<_>>()
        );
        drop(list);

        let _ = rx
            .recv_timeout(std::time::Duration::from_millis(200))
            .map(|(topic, _)| {
                panic!("zero jobs may start — unexpected extra event: {topic}");
            });
    }

    fn make_app(
        root: &std::path::Path,
        settings_json: serde_json::Value,
    ) -> tauri::App<tauri::test::MockRuntime> {
        make_app_inner(root, settings_json, true)
    }

    fn make_app_inner(
        root: &std::path::Path,
        settings_json: serde_json::Value,
        manage_history: bool,
    ) -> tauri::App<tauri::test::MockRuntime> {
        let settings_store = root.join("settings.json");
        std::fs::write(
            &settings_store,
            serde_json::json!({ "settings": settings_json }).to_string(),
        )
        .unwrap();

        let mut ctx = tauri::test::mock_context(tauri::test::noop_assets());
        ctx.config_mut().app.app_directories_override =
            Some(tauri::utils::config::AppDirectoriesOverride::Root(
                root.to_path_buf(),
            ));
        ctx.config_mut().identifier = "com.app.test".into();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .manage(FileListState(std::sync::Mutex::new(Vec::new())))
            .build(ctx)
            .unwrap();
        app.manage(QueueState::new(app.handle().clone()));
        if manage_history {
            app.manage(crate::history::HistoryStore::new_in(root));
        }
        app
    }

    fn fixed_estimate(category: crate::models::Category, _input: u64, _width: Option<u32>) -> u64 {
        const MIB: u64 = 1024 * 1024;
        match category {
            crate::models::Category::Video => 600 * MIB,
            _ => 300 * MIB,
        }
    }

    fn setup_app(
        budget_mb: u32,
    ) -> (
        tempfile::TempDir,
        tauri::App<tauri::test::MockRuntime>,
        std::path::PathBuf,
    ) {
        let root = tempfile::TempDir::new().unwrap();
        let out = root.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let app = make_app(
            root.path(),
            serde_json::json!({
                "outputFolder": out.to_str().unwrap(),
                "performance": { "adaptiveMemory": false, "memoryCeilingMb": budget_mb }
            }),
        );
        app.state::<QueueState<tauri::test::MockRuntime>>()
            .set_test_estimate(fixed_estimate);
        (root, app, out)
    }

    static TEMP_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn temp_guard() -> std::sync::MutexGuard<'static, ()> {
        TEMP_GUARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn add_entries(
        app: &tauri::App<tauri::test::MockRuntime>,
        specs: &[(&str, crate::models::Category, &str)],
    ) -> Vec<String> {
        use crate::models::FileEntry;
        let state = app.state::<FileListState>();
        let mut list = state.0.lock().unwrap();
        specs
            .iter()
            .map(|(name, category, path)| {
                let mut entry = FileEntry::new(path, 1 << 20);
                entry.name = name.to_string();
                entry.category = *category;
                entry.status = crate::models::FileStatus::Pending;
                list.push(entry.clone());
                entry.id
            })
            .collect()
    }

    fn events_of(
        app: &tauri::AppHandle<tauri::test::MockRuntime>,
        topics: &[&str],
    ) -> std::sync::mpsc::Receiver<(String, serde_json::Value)> {
        use tauri::Listener;
        let (tx, rx) = std::sync::mpsc::channel::<(String, serde_json::Value)>();
        for topic in topics {
            let tx = tx.clone();
            let topic = topic.to_string();
            let _ = app.listen(topic.clone(), move |event: tauri::Event| {
                let _ = tx.send((
                    topic.clone(),
                    serde_json::from_str(event.payload()).unwrap(),
                ));
            });
        }
        rx
    }

    fn wait_idle(queue: &QueueState<tauri::test::MockRuntime>, batch_id: &str) {
        for _ in 0..250 {
            if !queue.batch_token_present(batch_id) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("batch did not finish in time");
    }

    #[tokio::test]
    async fn tight_budget_runs_one_job_at_a_time() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(450);
        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
            ("b.pdf", crate::models::Category::Pdf, "/tmp/b.pdf"),
            ("c.mp4", crate::models::Category::Video, "/tmp/c.mp4"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "file:complete", "file:cancelled", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "b1".into(), ids.clone());

        let mut order = Vec::new();
        for _ in 0..3 {
            let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            assert_eq!(topic, "file:error", "missing sidecars fail every job; got {topic} {payload}");
            order.push(payload["fileId"].as_str().unwrap().to_string());
        }
        assert_eq!(order, ids, "one-job-at-a-time order must equal batch order");
        let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "batch:complete");
    }

    #[tokio::test]
    async fn video_estimate_blocks_image_until_lane_drains() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(600);
        let ids = add_entries(&app, &[
            ("v1.mp4", crate::models::Category::Video, "/tmp/v1.mp4"),
            ("v2.mp4", crate::models::Category::Video, "/tmp/v2.mp4"),
            ("i1.jpg", crate::models::Category::Image, "/tmp/i1.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "b2".into(), ids.clone());

        let mut order = Vec::new();
        for _ in 0..3 {
            let (_, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            order.push(payload["fileId"].as_str().unwrap().to_string());
        }
        assert_eq!(order, ids, "a 600 MiB video estimate fills the whole 600 MiB budget");
    }

    #[tokio::test]
    async fn cancel_all_mid_batch_marks_pending_cancelled_across_lanes() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(900);
        let ids = add_entries(&app, &[
            ("i1.jpg", crate::models::Category::Image, "/tmp/i1.jpg"),
            ("i2.jpg", crate::models::Category::Image, "/tmp/i2.jpg"),
            ("p1.pdf", crate::models::Category::Pdf, "/tmp/p1.pdf"),
            ("p2.pdf", crate::models::Category::Pdf, "/tmp/p2.pdf"),
            ("v1.mp4", crate::models::Category::Video, "/tmp/v1.mp4"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "file:cancelled", "file:complete", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "b3".into(), ids.clone());

        let (topic, first) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "file:error");
        let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        seen.insert(
            first["fileId"].as_str().unwrap().to_string(),
            "file:error".to_string(),
        );
        queue.cancel_all();

        wait_idle(&queue, "b3");
        loop {
            let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            if topic == "batch:complete" {
                break;
            }
            let id = payload["fileId"].as_str().unwrap().to_string();
            assert!(
                !seen.contains_key(&id),
                "each id must be finalized exactly once, {id} repeated as {topic}"
            );
            seen.insert(id, topic);
        }
        let covered: std::collections::HashSet<String> = seen.keys().cloned().collect();
        assert_eq!(
            covered,
            ids.iter().cloned().collect::<std::collections::HashSet<String>>(),
            "file:error + file:cancelled must cover all 5 ids exactly once each, seen: {seen:?}"
        );

        let state = app.state::<FileListState>();
        let list = state.0.lock().unwrap();
        for entry in list.iter() {
            match seen.get(&entry.id).map(|s| s.as_str()) {
                Some("file:error") => assert_eq!(
                    entry.status,
                    crate::models::FileStatus::Failed,
                    "row finalized with file:error must be Failed: {}",
                    entry.name
                ),
                _ => assert_eq!(
                    entry.status,
                    crate::models::FileStatus::Cancelled,
                    "row not seen in file:error must be Cancelled (not Failed/Pending): {} → {:?}",
                    entry.name,
                    entry.status
                ),
            }
        }
    }

    #[tokio::test]
    async fn cancel_all_across_lanes_cleans_each_jobs_temp_and_frees_budget() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(900);
        let ids = add_entries(&app, &[
            ("i1.jpg", crate::models::Category::Image, "/tmp/i1.jpg"),
            ("i2.jpg", crate::models::Category::Image, "/tmp/i2.jpg"),
            ("p1.pdf", crate::models::Category::Pdf, "/tmp/p1.pdf"),
            ("p2.pdf", crate::models::Category::Pdf, "/tmp/p2.pdf"),
            ("v1.mp4", crate::models::Category::Video, "/tmp/v1.mp4"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "file:cancelled", "file:complete", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();

        let app_temp = std::env::temp_dir().join("yard");
        let snapshot = |dir: &std::path::Path| -> std::collections::HashSet<String> {
            std::fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .collect()
                })
                .unwrap_or_default()
        };
        let before = snapshot(&app_temp);

        QueueState::start_batch(queue.inner(), &app.handle(), "b7".into(), ids.clone());

        let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "file:error");
        queue.cancel_all();

        wait_idle(&queue, "b7");
        loop {
            let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            if topic == "batch:complete" {
                break;
            }
        }

        let after = snapshot(&app_temp);
        let leftovers: Vec<&String> = after.difference(&before).collect();
        assert!(
            leftovers.is_empty(),
            "cancelled batch must leave no orphan session temp dirs under {app_temp:?}: {leftovers:?}"
        );

        let state = app.state::<FileListState>();
        let list = state.0.lock().unwrap();
        assert!(
            list.iter().all(|e| e.status != crate::models::FileStatus::Pending),
            "no row may stay Pending after cancel-all, got: {:?}",
            list.iter().map(|e| (e.name.clone(), e.status.clone())).collect::<Vec<_>>()
        );
        drop(list);

        let second = add_entries(&app, &[("solo.jpg", crate::models::Category::Image, "/tmp/solo.jpg")]);
        QueueState::start_batch(queue.inner(), &app.handle(), "b7-second".into(), second);
        wait_idle(&queue, "b7-second");
        loop {
            let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            if topic == "batch:complete" {
                break;
            }
        }
    }

    #[tokio::test]
    async fn single_failure_does_not_stop_lane_or_batch() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(900);
        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
            ("b.pdf", crate::models::Category::Pdf, "/tmp/b.pdf"),
            ("c.mp4", crate::models::Category::Video, "/tmp/c.mp4"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "b4".into(), ids.clone());

        for _ in 0..3 {
            let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            assert_eq!(topic, "file:error", "every file still processes despite the first failure");
        }
        let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "batch:complete");
        assert!(payload["totalSavedBytes"].is_number());
    }

    #[tokio::test]
    async fn batch_complete_fires_once_after_all_lanes_drain() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(900);
        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
            ("b.pdf", crate::models::Category::Pdf, "/tmp/b.pdf"),
            ("c.mp4", crate::models::Category::Video, "/tmp/c.mp4"),
            ("d.jpg", crate::models::Category::Image, "/tmp/d.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "b5".into(), ids);
        wait_idle(&queue, "b5");

        let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "batch:complete");
        let extra = rx.recv_timeout(std::time::Duration::from_millis(300));
        assert!(extra.is_err(), "batch:complete must fire exactly once");
    }

    #[tokio::test]
    async fn parallel_same_name_outputs_get_suffixes_end_to_end() {
        use std::io::Write;

        let _temp_guard = temp_guard();
        let (root, app, _out) = setup_app(600);

        let mut docx_paths = Vec::new();
        for i in 0..2 {
            let src = root.path().join(format!("src{i}"));
            std::fs::create_dir_all(&src).unwrap();
            let path = src.join("report.docx");
            let file = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("[Content_Types].xml", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"<?xml version=\"1.0\"?><Types/>").unwrap();
            zip.finish().unwrap();
            docx_paths.push(path);
        }
        let ids = add_entries(&app, &[
            ("report.docx", crate::models::Category::Document, docx_paths[0].to_str().unwrap()),
            ("report.docx", crate::models::Category::Document, docx_paths[1].to_str().unwrap()),
        ]);

        let rx = events_of(&app.handle(), &["file:complete", "file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "b6".into(), ids);

        let mut output_names = Vec::new();
        for _ in 0..2 {
            let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            assert_eq!(topic, "file:complete", "zero-media docx must not fail: {payload}");
            output_names.push(payload["outputPath"].as_str().unwrap().to_string());
        }
        output_names.sort();
        let name = |s: &str| std::path::Path::new(s).file_name().unwrap().to_str().unwrap().to_string();
        let names: Vec<String> = output_names.iter().map(|s| name(s)).collect();
        assert_eq!(names, vec!["report-1.docx", "report.docx"], "parallel same-name claims must suffix");
    }

    fn drain_until_complete(rx: &std::sync::mpsc::Receiver<(String, serde_json::Value)>) {
        loop {
            let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            if topic == "batch:complete" {
                break;
            }
        }
    }

    fn raw_file_records(
        store: &crate::history::HistoryStore,
    ) -> Vec<crate::history::FileResultRecord> {
        store
            .read_lines()
            .unwrap()
            .iter()
            .filter_map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).ok()?;
                if value["type"] != "file" {
                    return None;
                }
                serde_json::from_value(value).ok()
            })
            .collect()
    }

    fn set_status(
        app: &tauri::App<tauri::test::MockRuntime>,
        file_id: &str,
        status: crate::models::FileStatus,
    ) {
        update_entry(app, file_id, |e| e.status = status);
    }

    fn update_entry(
        app: &tauri::App<tauri::test::MockRuntime>,
        file_id: &str,
        update: impl FnOnce(&mut FileEntry),
    ) {
        let state = app.state::<FileListState>();
        let mut list = state.0.lock().unwrap();
        let entry = list
            .iter_mut()
            .find(|e| e.id == file_id)
            .expect("entry must exist");
        update(entry);
    }

    fn seed_session_start(
        store: &crate::history::HistoryStore,
        session_id: &str,
        started_at: i64,
        output_folder: &str,
    ) {
        store
            .append_session_start(&crate::history::SessionStartRecord {
                session_id: session_id.into(),
                started_at,
                file_count: 1,
                output_folder: output_folder.into(),
            })
            .unwrap();
    }

    #[test]
    fn history_counts_map_all_statuses() {
        let rec = |status: &str| FileResultRecord {
            session_id: "s".into(),
            file_id: uuid::Uuid::new_v4().to_string(),
            completed_at: 0,
            name: "a".into(),
            path: "/a".into(),
            output_path: None,
            category: "image".into(),
            target_bytes: 1,
            original_size: 1,
            compressed_size: None,
            status: status.into(),
            quality_used: None,
            strategy_used: None,
            error: None,
            duration_ms: 0,
        };
        let files = vec![
            rec("done"),
            rec("best_effort"),
            rec("failed"),
            rec("cancelled"),
            rec("unsupported"),
            rec("already_under_target"),
            rec("pending"),
            rec("compressing"),
            rec("mystery"),
        ];
        let counts = history_counts_of(&files);
        assert_eq!(counts.done, 1);
        assert_eq!(counts.best_effort, 1);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.cancelled, 1);
        assert_eq!(counts.unsupported, 1);
        assert_eq!(counts.already_under_target, 1);
    }

    #[test]
    fn status_key_snake_case() {
        let all = [
            FileStatus::Pending,
            FileStatus::Compressing,
            FileStatus::Done,
            FileStatus::BestEffort,
            FileStatus::Failed,
            FileStatus::Cancelled,
            FileStatus::Unsupported,
            FileStatus::AlreadyUnderTarget,
        ];
        assert_eq!(all.len(), 8, "status_key must cover every FileStatus variant");
        for status in all {
            let expected = serde_json::to_value(status).unwrap();
            assert_eq!(
                serde_json::to_value(status_key(&status)).unwrap(),
                expected,
                "history status strings must match serde snake_case for {status:?}"
            );
        }
        assert_eq!(status_key(&FileStatus::AlreadyUnderTarget), "already_under_target");
        assert_eq!(status_key(&FileStatus::Pending), "pending");
        assert_eq!(status_key(&FileStatus::Compressing), "compressing");
    }

    #[test]
    fn category_key_snake_case() {
        use crate::models::Category;
        let all = [
            Category::Image,
            Category::Video,
            Category::Pdf,
            Category::Document,
            Category::Other,
        ];
        assert_eq!(all.len(), 5, "category_key must cover every Category variant");
        for category in all {
            let expected = serde_json::to_value(category).unwrap();
            assert_eq!(
                serde_json::to_value(category_key(&category)).unwrap(),
                expected,
                "history category strings must match serde lowercase for {category:?}"
            );
        }
    }

    #[test]
    fn file_complete_payload_carries_strategy_fields() {
        let payload = FileCompletePayload {
            file_id: "f1",
            output_path: "out".into(),
            original_size: 2,
            compressed_size: 1,
            status: FileStatus::Done,
            met_target: true,
            quality_used: Some(70),
            strategy_used: Some("quality=70".into()),
        };
        let v = serde_json::to_value(&payload).unwrap();
        assert_eq!(v["qualityUsed"], 70);
        assert_eq!(v["strategyUsed"], "quality=70");
    }

    #[tokio::test]
    async fn lane_pending_cancelled_entry_recorded_once() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(900);
        let ids = add_entries(&app, &[
            ("i1.jpg", crate::models::Category::Image, "/tmp/i1.jpg"),
            ("i2.jpg", crate::models::Category::Image, "/tmp/i2.jpg"),
            ("p1.pdf", crate::models::Category::Pdf, "/tmp/p1.pdf"),
        ]);
        set_status(&app, &ids[1], crate::models::FileStatus::Cancelled);

        let store = app.state::<crate::history::HistoryStore>();
        let store = store.inner();
        let rx = events_of(&app.handle(), &["file:error", "file:cancelled", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-lane-cancel".into(), ids.clone());
        wait_idle(&queue, "hist-lane-cancel");

        let mut cancelled_events = 0usize;
        loop {
            let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            if topic == "file:cancelled" {
                cancelled_events += 1;
            }
            if topic == "batch:complete" {
                break;
            }
        }
        assert_eq!(
            cancelled_events, 0,
            "the lane sweep must not re-emit file:cancelled (commands::cancel_file owns that event)"
        );

        let recorded: Vec<String> = raw_file_records(store)
            .iter()
            .map(|r| r.file_id.clone())
            .collect();
        let recorded_set: std::collections::HashSet<&String> = recorded.iter().collect();
        let expected: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(
            recorded_set, expected,
            "one file record per batch id, got {recorded:?}"
        );
        assert_eq!(recorded.len(), ids.len(), "no duplicate file records");

        let fold = store.load().unwrap();
        let s = &fold.sessions[0];
        assert_eq!(
            s.outcome.cancelled, 1,
            "the pre-cancelled pending file must be counted cancelled"
        );
        assert_eq!(
            s.outcome.failed + s.outcome.cancelled,
            u32::try_from(ids.len()).unwrap(),
            "every file terminal outcome accounted"
        );
        assert_eq!(s.files.len(), ids.len());
    }

    #[tokio::test]
    async fn queued_entry_individually_cancelled_records_on_dispatch() {
        let _temp_guard = temp_guard();
        let (_root, app, out) = setup_app(900);
        let ids = add_entries(&app, &[
            ("queued.jpg", crate::models::Category::Image, "/tmp/queued.jpg"),
        ]);
        set_status(&app, &ids[0], crate::models::FileStatus::Cancelled);

        let store = app.state::<crate::history::HistoryStore>();
        let store = store.inner();
        let settings = crate::commands::load_settings(app.handle());
        let reserved = crate::output::ReservedNames::new();
        let rx = events_of(&app.handle(), &["file:cancelled"]);
        let saved = QueueState::process_single_job(
            app.handle().clone(),
            "hist-queued-cancel".into(),
            ids[0].clone(),
            out.to_str().unwrap().to_string(),
            settings,
            &reserved,
        )
        .await;

        assert_eq!(saved, 0, "a cancelled entry must contribute no saved bytes");
        assert!(
            rx.try_recv().is_err(),
            "dispatch of an already-cancelled entry must not re-emit file:cancelled"
        );
        let recorded = raw_file_records(store);
        assert_eq!(recorded.len(), 1, "dispatch of a cancelled entry records once");
        let f = &recorded[0];
        assert_eq!(f.file_id, ids[0]);
        assert_eq!(f.status, "cancelled");
        assert_eq!(f.duration_ms, 0, "cancelled before a worker slot means zero duration");
        assert!(f.output_path.is_none());
        assert!(f.compressed_size.is_none());
        assert!(f.quality_used.is_none());
        assert!(f.strategy_used.is_none());
    }

    #[tokio::test]
    async fn failed_batch_writes_session_records() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(450);
        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
            ("b.jpg", crate::models::Category::Image, "/tmp/b.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-1".into(), ids.clone());
        wait_idle(&queue, "hist-1");

        let mut emitted_errors: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        loop {
            let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            if topic == "file:error" {
                let file_id = payload["fileId"].as_str().unwrap().to_string();
                let message = payload["errorMessage"].as_str().unwrap().to_string();
                assert!(!message.is_empty(), "file:error must carry a message");
                emitted_errors.insert(file_id, message);
            }
            if topic == "batch:complete" {
                break;
            }
        }
        assert_eq!(emitted_errors.len(), 2, "one file:error per failed file");

        let store = app.state::<crate::history::HistoryStore>();
        let fold = store.load().unwrap();
        assert_eq!(fold.sessions.len(), 1);
        let s = &fold.sessions[0];
        assert_eq!(s.session_id, "hist-1");
        assert_eq!(s.file_count, 2);
        assert!(!s.interrupted, "session_end must be written by finish_batch");
        assert!(!s.cancelled, "a batch that ran to completion is not cancelled");
        assert_eq!(s.outcome.failed, 2, "sidecar-less jobs fail");
        assert_eq!(s.files.len(), 2);
        let raw = raw_file_records(store.inner());
        assert_eq!(raw.len(), 2, "one raw file record per failed file");
        for f in &raw {
            let error = f
                .error
                .as_ref()
                .unwrap_or_else(|| panic!("failed record {} must carry an error", f.file_id));
            assert!(!error.is_empty(), "failed record error must be non-empty");
            assert_eq!(
                Some(error),
                emitted_errors.get(&f.file_id),
                "recorded error for {} must match the emitted file:error errorMessage",
                f.file_id
            );
        }
        assert_eq!(
            s.files.iter().map(|f| f.name.as_str()).collect::<std::collections::HashSet<_>>(),
            std::collections::HashSet::from(["a.jpg", "b.jpg"])
        );
        let started_at = s.started_at;
        let ended_at = s.ended_at.expect("session_end carries ended_at");
        assert!(started_at > 0, "session start must carry a real timestamp");
        assert!(ended_at >= started_at, "session end must not precede session start");
        for f in &s.files {
            let completed_at = f.completed_at;
            assert!(
                completed_at >= started_at && completed_at <= ended_at,
                "file {completed_at} must fall within [{started_at}, {ended_at}]"
            );
        }
    }

    #[tokio::test]
    async fn session_end_totals_match_accumulated_records() {
        use std::io::Write;

        let _temp_guard = temp_guard();
        let (root, app, out) = setup_app(450);

        let src = root.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let docx = src.join("report.docx");
        let file = std::fs::File::create(&docx).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("[Content_Types].xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"<?xml version=\"1.0\"?><Types/>").unwrap();
        zip.finish().unwrap();

        let ids = add_entries(&app, &[
            ("report.docx", crate::models::Category::Document, docx.to_str().unwrap()),
            ("b.jpg", crate::models::Category::Image, "/tmp/b.jpg"),
        ]);
        update_entry(&app, &ids[0], |entry| entry.original_size = 4096);
        let docx_id = ids[0].clone();
        let on_disk = std::fs::metadata(&docx).unwrap().len();
        let expected_saved = 4096u64.saturating_sub(on_disk);

        let rx = events_of(&app.handle(), &["file:complete", "file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-totals".into(), ids);
        wait_idle(&queue, "hist-totals");

        let mut completes: Vec<(String, serde_json::Value)> = Vec::new();
        let mut errors: Vec<(String, serde_json::Value)> = Vec::new();
        let batch_complete = loop {
            let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            match topic.as_str() {
                "file:complete" => {
                    let file_id = payload["fileId"].as_str().unwrap().to_string();
                    completes.push((file_id, payload));
                }
                "file:error" => {
                    let file_id = payload["fileId"].as_str().unwrap().to_string();
                    errors.push((file_id, payload));
                }
                "batch:complete" => break payload,
                _ => {}
            }
        };
        assert_eq!(completes.len(), 1, "the under-target docx emits file:complete");
        assert_eq!(errors.len(), 1, "the missing jpg emits file:error");
        let docx_complete = completes
            .iter()
            .find(|(file_id, _)| file_id == &docx_id)
            .expect("file:complete for the docx");
        assert_eq!(
            docx_complete.1["qualityUsed"].as_u64(),
            Some(100),
            "file:complete must carry the worker's exact quality: {}",
            docx_complete.1
        );
        assert_eq!(
            docx_complete.1["strategyUsed"].as_str(),
            Some(crate::engine::result::STRATEGY_ALREADY_UNDER_TARGET),
            "file:complete must carry the worker's exact strategy: {}",
            docx_complete.1
        );

        let store = app.state::<crate::history::HistoryStore>();
        let fold = store.load().unwrap();
        let s = &fold.sessions[0];
        assert_eq!(s.files.len(), 2);
        assert_eq!(s.outcome.already_under_target, 1, "tiny docx is already under target");
        assert_eq!(s.outcome.failed, 1);
        assert_eq!(s.outcome.done, 0);
        assert_eq!(s.outcome.best_effort, 0);
        assert_eq!(s.outcome.cancelled, 0);
        assert_eq!(s.outcome.unsupported, 0);
        assert_eq!(
            s.total_saved_bytes, expected_saved,
            "session_end total must equal the worker-accumulated saved bytes ({expected_saved})"
        );
        assert_eq!(
            batch_complete["totalSavedBytes"].as_u64(),
            Some(expected_saved),
            "batch:complete must carry the same total as session_end: {batch_complete}"
        );
        assert!(
            s.total_saved_bytes > 0,
            "fixture must accumulate a non-zero saved amount so a zero total cannot pass"
        );
        let processed = s.files.iter().find(|f| f.status == "already_under_target").unwrap();
        assert_eq!(processed.category, "document");
        assert_eq!(processed.original_size, 4096);
        assert_eq!(processed.compressed_size, Some(on_disk));
        assert_eq!(
            processed.output_path,
            Some(out.join("report.docx")),
            "the recorded output path is the exact resolved destination"
        );
        assert_eq!(processed.quality_used, Some(100));
        assert_eq!(
            processed.strategy_used.as_deref(),
            Some(crate::engine::result::STRATEGY_ALREADY_UNDER_TARGET)
        );
        assert!(!s.interrupted);
    }

    #[tokio::test]
    async fn preflight_failure_writes_nothing() {
        let _temp_guard = temp_guard();
        let root = tempfile::TempDir::new().unwrap();
        let blocker = root.path().join("blocker.txt");
        std::fs::write(&blocker, b"x").unwrap();
        let output_folder = blocker.join("child");
        let app = make_app(
            root.path(),
            serde_json::json!({ "outputFolder": output_folder.to_str().unwrap() }),
        );

        let entry = crate::models::FileEntry::new("x.jpg", 1024);
        app.state::<FileListState>().0.lock().unwrap().push(entry.clone());
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        queue.seed_history_state("preflight");
        QueueState::start_batch(queue.inner(), &app.handle(), "preflight".into(), vec![entry.id.clone()]);
        wait_idle(&queue, "preflight");

        assert!(
            !queue.history_records_present("preflight"),
            "no per-batch records may survive a preflight failure"
        );
        assert!(
            !queue.history_error_pending("preflight"),
            "no per-batch error flag may survive a preflight failure"
        );

        let store = app.state::<crate::history::HistoryStore>();
        assert!(store.load().unwrap().sessions.is_empty(), "no phantom session");
    }

    #[tokio::test]
    async fn batch_fails_when_output_disk_is_short() {
        let _temp_guard = temp_guard();
        let (root, app, _out) = setup_app(450);
        let source = root.path().join("big.jpg");
        std::fs::write(&source, vec![7u8; 2 * 1024 * 1024]).unwrap();
        let ids = add_entries(
            &app,
            &[(
                "big.jpg",
                crate::models::Category::Image,
                source.to_str().unwrap(),
            )],
        );
        let rx = events_of(
            &app.handle(),
            &["batch:error", "batch:complete", "file:error"],
        );
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        queue.set_test_available_space(|_| Some(0));
        queue.seed_history_state("disk-short");
        QueueState::start_batch(queue.inner(), &app.handle(), "disk-short".into(), ids);
        wait_idle(&queue, "disk-short");

        let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "batch:error");
        assert!(
            payload["errorMessage"]
                .as_str()
                .unwrap()
                .contains("Insufficient disk space"),
            "disk preflight must fail the batch: {payload}"
        );
        assert!(
            rx.try_recv().is_err(),
            "a preflight failure must emit no job or completion events"
        );
        assert!(
            !queue.history_records_present("disk-short"),
            "no per-batch records may survive a disk preflight failure"
        );
        assert!(
            !queue.history_error_pending("disk-short"),
            "no per-batch error flag may survive a disk preflight failure"
        );

        let store = app.state::<crate::history::HistoryStore>();
        assert!(
            store.load().unwrap().sessions.is_empty(),
            "a disk preflight failure must write no session record"
        );
    }

    #[test]
    fn cancel_skip_entry_ignores_non_pending() {
        let entry = |id: &str, status: FileStatus| {
            let mut e = FileEntry::new(&format!("/tmp/{id}"), 1024);
            e.id = id.to_string();
            e.status = status;
            e
        };
        let mut list = vec![
            entry("done", FileStatus::Done),
            entry("failed", FileStatus::Failed),
            entry("cancelled", FileStatus::Cancelled),
            entry("pending", FileStatus::Pending),
        ];
        assert!(cancel_skip_entry(&mut list, "done").is_none(), "Done must not be swept");
        assert!(cancel_skip_entry(&mut list, "failed").is_none(), "Failed must not be swept");
        assert!(cancel_skip_entry(&mut list, "missing").is_none(), "unknown ids are no-ops");

        let already = cancel_skip_entry(&mut list, "cancelled").expect("Cancelled is returned");
        assert_eq!(already.status, FileStatus::Cancelled);
        let pending = cancel_skip_entry(&mut list, "pending").expect("Pending flips to Cancelled");
        assert_eq!(pending.status, FileStatus::Cancelled);

        assert_eq!(
            list.iter().find(|e| e.id == "done").unwrap().status,
            FileStatus::Done,
            "non-pending rows are never clobbered"
        );
        assert_eq!(
            list.iter().find(|e| e.id == "failed").unwrap().status,
            FileStatus::Failed
        );
    }

    #[tokio::test]
    async fn done_entry_not_recorded_on_skip_path() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(900);
        let ids = add_entries(&app, &[
            ("stale.jpg", crate::models::Category::Image, "/tmp/stale.jpg"),
            ("fresh.jpg", crate::models::Category::Image, "/tmp/fresh.jpg"),
        ]);
        set_status(&app, &ids[0], crate::models::FileStatus::Done);

        let rx = events_of(&app.handle(), &["file:error", "file:cancelled", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-stale".into(), ids.clone());
        wait_idle(&queue, "hist-stale");
        drain_until_complete(&rx);

        let store = app.state::<crate::history::HistoryStore>();
        let recorded: Vec<String> = raw_file_records(&store).iter().map(|r| r.file_id.clone()).collect();
        assert_eq!(
            recorded,
            vec![ids[1].clone()],
            "the Done row is skipped by the lane sweep and only the live file is recorded"
        );

        let state = app.state::<FileListState>();
        let list = state.0.lock().unwrap();
        let stale = list.iter().find(|e| e.id == ids[0]).unwrap();
        assert_eq!(
            stale.status,
            crate::models::FileStatus::Done,
            "the sweep must not clobber a Done row"
        );
    }

    #[tokio::test]
    async fn cancel_all_records_cancelled_files() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(900);
        let ids = add_entries(&app, &[
            ("i1.jpg", crate::models::Category::Image, "/tmp/i1.jpg"),
            ("i2.jpg", crate::models::Category::Image, "/tmp/i2.jpg"),
            ("p1.pdf", crate::models::Category::Pdf, "/tmp/p1.pdf"),
            ("p2.pdf", crate::models::Category::Pdf, "/tmp/p2.pdf"),
            ("v1.mp4", crate::models::Category::Video, "/tmp/v1.mp4"),
            ("v2.mp4", crate::models::Category::Video, "/tmp/v2.mp4"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "file:cancelled", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-cancel".into(), ids.clone());
        let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "file:error");
        queue.cancel_all();
        wait_idle(&queue, "hist-cancel");
        drain_until_complete(&rx);

        let store = app.state::<crate::history::HistoryStore>();
        let recorded: Vec<String> = raw_file_records(&store).iter().map(|r| r.file_id.clone()).collect();
        assert_eq!(
            recorded.iter().cloned().collect::<std::collections::HashSet<_>>(),
            ids.iter().cloned().collect::<std::collections::HashSet<_>>(),
            "recorded file ids must equal the batch ids exactly, got {recorded:?}"
        );
        assert_eq!(recorded.len(), ids.len(), "no duplicate file records");

        let fold = store.load().unwrap();
        let s = &fold.sessions[0];
        assert_eq!(s.session_id, "hist-cancel");
        assert_eq!(s.files.len(), ids.len(), "every file terminal outcome recorded once");
        assert!(s.files.iter().any(|f| f.status == "failed"));
        assert!(s.files.iter().any(|f| f.status == "cancelled"));
        assert_eq!(
            s.outcome.failed + s.outcome.cancelled,
            u32::try_from(ids.len()).unwrap(),
            "counts must cover every recorded file"
        );
        for f in s.files.iter().filter(|f| f.status == "cancelled") {
            assert_eq!(
                f.duration_ms, 0,
                "cancelled-before-slot records carry zero duration: {f:?}"
            );
        }
        assert!(s.cancelled, "batch ended via cancel-all");
    }

    #[tokio::test]
    async fn batch_complete_payload_carries_cancelled_flag() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(450);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();

        let ids = add_entries(&app, &[("a.jpg", crate::models::Category::Image, "/tmp/a.jpg")]);
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        QueueState::start_batch(queue.inner(), &app.handle(), "b-natural".into(), ids);
        let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "file:error");
        let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "batch:complete");
        assert_eq!(payload["batchId"], "b-natural");
        assert!(
            payload["totalSavedBytes"].is_u64(),
            "complete payload carries the saved total: {payload}"
        );
        assert_eq!(
            payload["cancelled"], false,
            "natural completion must carry cancelled:false: {payload}"
        );

        let ids = add_entries(&app, &[
            ("i1.jpg", crate::models::Category::Image, "/tmp/i1.jpg"),
            ("i2.jpg", crate::models::Category::Image, "/tmp/i2.jpg"),
            ("p1.pdf", crate::models::Category::Pdf, "/tmp/p1.pdf"),
            ("p2.pdf", crate::models::Category::Pdf, "/tmp/p2.pdf"),
            ("v1.mp4", crate::models::Category::Video, "/tmp/v1.mp4"),
            ("v2.mp4", crate::models::Category::Video, "/tmp/v2.mp4"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        QueueState::start_batch(queue.inner(), &app.handle(), "b-cancel".into(), ids);
        let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "file:error");
        queue.cancel_all();
        wait_idle(&queue, "b-cancel");
        let (topic, payload) = loop {
            let (topic, payload) =
                rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            if topic == "batch:complete" {
                break (topic, payload);
            }
        };
        assert_eq!(payload["batchId"], "b-cancel");
        assert_eq!(
            payload["cancelled"], true,
            "cancel-all mid-batch must carry cancelled:true: {payload}"
        );
    }

    #[tokio::test]
    async fn sequential_cancel_all_records_cancelled() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(450);
        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
            ("b.jpg", crate::models::Category::Image, "/tmp/b.jpg"),
            ("c.jpg", crate::models::Category::Image, "/tmp/c.jpg"),
            ("d.jpg", crate::models::Category::Image, "/tmp/d.jpg"),
            ("e.jpg", crate::models::Category::Image, "/tmp/e.jpg"),
            ("f.jpg", crate::models::Category::Image, "/tmp/f.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "file:cancelled", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-seq".into(), ids.clone());
        let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "file:error");
        queue.cancel_all();
        wait_idle(&queue, "hist-seq");
        drain_until_complete(&rx);

        let store = app.state::<crate::history::HistoryStore>();
        let recorded: Vec<String> = raw_file_records(&store).iter().map(|r| r.file_id.clone()).collect();
        assert_eq!(
            recorded.iter().cloned().collect::<std::collections::HashSet<_>>(),
            ids.iter().cloned().collect::<std::collections::HashSet<_>>(),
            "recorded file ids must equal the batch ids exactly, got {recorded:?}"
        );
        assert_eq!(recorded.len(), ids.len(), "no duplicate file records");

        let fold = store.load().unwrap();
        let s = &fold.sessions[0];
        assert_eq!(s.files.len(), ids.len(), "every file terminal outcome recorded once");
        let failed = s.files.iter().filter(|f| f.status == "failed").count();
        let cancelled = s.files.iter().filter(|f| f.status == "cancelled").count();
        assert!(cancelled >= 1, "remaining queue must be recorded cancelled: {failed} failed / {cancelled} cancelled");
        assert_eq!(failed + cancelled, ids.len());
        for f in s.files.iter().filter(|f| f.status == "cancelled") {
            assert_eq!(
                f.duration_ms, 0,
                "cancelled-before-slot records carry zero duration: {f:?}"
            );
        }
        assert_eq!(s.outcome.failed + s.outcome.cancelled, ids.len() as u32);
        assert!(s.cancelled);
    }

    #[tokio::test]
    async fn cancel_skipped_records_have_zero_duration_and_no_output() {
        let _temp_guard = temp_guard();
        let (root, app, _out) = setup_app(900);
        let real = root.path().join("real.jpg");
        std::fs::write(&real, vec![7u8; 1024]).unwrap();
        let ids = add_entries(&app, &[
            ("real.jpg", crate::models::Category::Image, real.to_str().unwrap()),
            ("i2.jpg", crate::models::Category::Image, "/tmp/missing-i2.jpg"),
            ("p1.pdf", crate::models::Category::Pdf, "/tmp/missing-p1.pdf"),
            ("p2.pdf", crate::models::Category::Pdf, "/tmp/missing-p2.pdf"),
            ("v1.mp4", crate::models::Category::Video, "/tmp/missing-v1.mp4"),
            ("v2.mp4", crate::models::Category::Video, "/tmp/missing-v2.mp4"),
        ]);
        set_status(&app, &ids[0], crate::models::FileStatus::Cancelled);
        let entries: Vec<FileEntry> = {
            let state = app.state::<FileListState>();
            let list = state.0.lock().unwrap();
            list.clone()
        };

        let rx = events_of(&app.handle(), &["file:error", "file:cancelled", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-skip".into(), ids.clone());
        let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(topic, "file:error");
        queue.cancel_all();
        wait_idle(&queue, "hist-skip");
        drain_until_complete(&rx);

        let store = app.state::<crate::history::HistoryStore>();
        let recorded = raw_file_records(&store);
        let recorded_ids: Vec<&str> = recorded.iter().map(|r| r.file_id.as_str()).collect();
        assert_eq!(
            recorded_ids.len(),
            ids.len(),
            "terminal outcome exactly once per file, got {recorded_ids:?}"
        );
        assert_eq!(
            recorded_ids.iter().cloned().collect::<std::collections::HashSet<_>>(),
            ids.iter().map(String::as_str).collect::<std::collections::HashSet<_>>(),
            "no duplicates and no missing ids"
        );

        let skipped: Vec<_> = recorded.iter().filter(|f| f.status == "cancelled").collect();
        assert!(!skipped.is_empty(), "cancel-all must record at least one skipped file");
        let real_record = skipped
            .iter()
            .find(|f| f.file_id == ids[0])
            .expect("pre-cancelled real-file entry must be recorded");
        assert_eq!(
            real_record.original_size, 1024,
            "sizes come from the entry, not from disk again"
        );
        assert_eq!(real_record.target_bytes, 1 << 20);
        assert_eq!(
            real_record.duration_ms, 0,
            "cancelled-before-slot records must carry zero duration (spec §3.2)"
        );

        for f in &skipped {
            let entry = entries.iter().find(|e| e.id == f.file_id).unwrap();
            assert!(f.output_path.is_none());
            assert!(f.compressed_size.is_none());
            assert_eq!(f.original_size, entry.original_size, "recorded size is the entry's size");
            assert_eq!(f.target_bytes, entry.target_bytes);
        }
    }

    #[tokio::test]
    async fn unsupported_outcome_recorded() {
        let _temp_guard = temp_guard();
        let (root, app, _out) = setup_app(450);
        let path = root.path().join("weird.xyz");
        std::fs::write(&path, vec![1u8; 512]).unwrap();
        let ids = add_entries(&app, &[
            ("weird.xyz", crate::models::Category::Other, path.to_str().unwrap()),
        ]);
        let rx = events_of(&app.handle(), &["file:complete", "file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-unsupported".into(), ids);
        wait_idle(&queue, "hist-unsupported");
        drain_until_complete(&rx);

        let store = app.state::<crate::history::HistoryStore>();
        let fold = store.load().unwrap();
        let s = &fold.sessions[0];
        assert_eq!(s.files.len(), 1);
        let f = &s.files[0];
        assert_eq!(f.status, "unsupported");
        assert_eq!(f.category, "other");
        assert!(f.output_path.is_none());
        assert!(f.compressed_size.is_none());
        assert!(f.quality_used.is_none());
        assert!(f.strategy_used.is_none());
        assert!(f.error.is_none());
        assert!(f.duration_ms > 0, "unsupported is detected after a worker slot: {}ms", f.duration_ms);
        assert_eq!(s.outcome.unsupported, 1);
        assert_eq!(f.original_size, 512);
    }

    #[tokio::test]
    async fn history_write_failure_does_not_fail_batch() {
        let _temp_guard = temp_guard();
        let root = tempfile::TempDir::new().unwrap();
        let out = root.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::create_dir_all(root.path().join("history.jsonl")).unwrap();
        let app = make_app(
            root.path(),
            serde_json::json!({
                "outputFolder": out.to_str().unwrap(),
                "performance": { "adaptiveMemory": false, "memoryCeilingMb": 450 }
            }),
        );
        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
            ("b.jpg", crate::models::Category::Image, "/tmp/b.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "history:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-fail".into(), ids.clone());
        wait_idle(&queue, "hist-fail");

        let mut file_errors = 0usize;
        let mut history_errors = 0usize;
        loop {
            let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            match topic.as_str() {
                "file:error" => file_errors += 1,
                "history:error" => {
                    history_errors += 1;
                    assert!(payload["message"].is_string());
                }
                "batch:complete" => break,
                other => panic!("unexpected event: {other}"),
            }
        }
        assert_eq!(file_errors, 2, "compression outcomes unchanged by history failure");
        assert_eq!(history_errors, 1, "persistent failure emits exactly one history:error");

        let state = app.state::<FileListState>();
        let list = state.0.lock().unwrap();
        assert!(
            list.iter().all(|e| e.status == crate::models::FileStatus::Failed),
            "file outcomes unchanged: {:?}",
            list.iter().map(|e| (e.name.clone(), e.status)).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn history_error_emitted_once_per_batch() {
        let _temp_guard = temp_guard();
        let root = tempfile::TempDir::new().unwrap();
        let out = root.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::create_dir_all(root.path().join("history.jsonl")).unwrap();
        let app = make_app(
            root.path(),
            serde_json::json!({
                "outputFolder": out.to_str().unwrap(),
                "performance": { "adaptiveMemory": false, "memoryCeilingMb": 900 }
            }),
        );
        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
            ("b.pdf", crate::models::Category::Pdf, "/tmp/b.pdf"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "history:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-once".into(), ids);
        wait_idle(&queue, "hist-once");

        let mut history_errors = 0usize;
        loop {
            let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            if topic == "history:error" {
                history_errors += 1;
                assert!(payload["message"].is_string());
            }
            if topic == "batch:complete" {
                break;
            }
        }
        assert_eq!(history_errors, 1, "at most one history:error per batch");
    }

    #[tokio::test]
    async fn history_error_flag_cleared_between_batches() {
        let _temp_guard = temp_guard();
        let root = tempfile::TempDir::new().unwrap();
        let out = root.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::create_dir_all(root.path().join("history.jsonl")).unwrap();
        let app = make_app(
            root.path(),
            serde_json::json!({
                "outputFolder": out.to_str().unwrap(),
                "performance": { "adaptiveMemory": false, "memoryCeilingMb": 450 }
            }),
        );
        let rx = events_of(&app.handle(), &["file:error", "history:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();

        let first = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
        ]);
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-two-1".into(), first);
        wait_idle(&queue, "hist-two-1");
        let second = add_entries(&app, &[
            ("b.jpg", crate::models::Category::Image, "/tmp/b.jpg"),
        ]);
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-two-2".into(), second);
        wait_idle(&queue, "hist-two-2");

        let mut history_errors = 0usize;
        let mut completes = 0usize;
        while completes < 2 {
            let (topic, payload) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            match topic.as_str() {
                "history:error" => {
                    history_errors += 1;
                    assert!(payload["message"].is_string());
                }
                "batch:complete" => completes += 1,
                _ => {}
            }
        }
        assert_eq!(
            history_errors, 2,
            "once-per-batch flag must reset so each failing batch reports its own history:error"
        );
        assert!(!queue.history_error_pending("hist-two-1"));
        assert!(!queue.history_error_pending("hist-two-2"));
    }

    #[tokio::test]
    async fn history_records_accumulator_cleared_after_batch() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(450);
        let first = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-acc-1".into(), first);
        wait_idle(&queue, "hist-acc-1");
        drain_until_complete(&rx);

        assert!(
            !queue.history_records_present("hist-acc-1"),
            "finish_batch must drain the per-batch accumulator"
        );

        let second = add_entries(&app, &[
            ("b.jpg", crate::models::Category::Image, "/tmp/b.jpg"),
        ]);
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-acc-2".into(), second);
        wait_idle(&queue, "hist-acc-2");
        drain_until_complete(&rx);

        let store = app.state::<crate::history::HistoryStore>();
        let fold = store.load().unwrap();
        let second_session = fold
            .sessions
            .iter()
            .find(|s| s.session_id == "hist-acc-2")
            .expect("second session recorded");
        assert_eq!(
            second_session.files.len(),
            1,
            "first batch's drained records must not leak into the second session"
        );
        assert_eq!(second_session.outcome.failed, 1);
    }

    #[tokio::test]
    async fn trim_failure_does_not_fail_batch() {
        let _temp_guard = temp_guard();
        let root = tempfile::TempDir::new().unwrap();
        let out = root.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let app = make_app(
            root.path(),
            serde_json::json!({
                "outputFolder": out.to_str().unwrap(),
                "performance": { "adaptiveMemory": false, "memoryCeilingMb": 450 },
                "history": { "retentionDays": 10 }
            }),
        );
        let store = app.state::<crate::history::HistoryStore>();
        seed_session_start(&store, "trim-old", now_ms() - 30 * 86_400_000, "/old");
        std::fs::create_dir_all(root.path().join("history.jsonl.tmp")).unwrap();

        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-trim-fail".into(), ids);
        wait_idle(&queue, "hist-trim-fail");
        drain_until_complete(&rx);

        let state = app.state::<FileListState>();
        let list = state.0.lock().unwrap();
        assert!(
            list.iter().all(|e| e.status == crate::models::FileStatus::Failed),
            "trim failure must not change file outcomes"
        );
        drop(list);

        let fold = store.load().unwrap();
        let ids: Vec<&str> = fold.sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert!(
            ids.contains(&"trim-old"),
            "failed trim must leave the original history intact, got {ids:?}"
        );
        assert!(ids.contains(&"hist-trim-fail"), "session_end written before trim");
    }

    #[tokio::test]
    async fn missing_store_still_completes_batch() {
        let _temp_guard = temp_guard();
        let root = tempfile::TempDir::new().unwrap();
        let out = root.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let app = make_app_inner(
            root.path(),
            serde_json::json!({
                "outputFolder": out.to_str().unwrap(),
                "performance": { "adaptiveMemory": false, "memoryCeilingMb": 900 }
            }),
            false,
        );
        assert!(app.try_state::<crate::history::HistoryStore>().is_none());

        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
            ("b.jpg", crate::models::Category::Image, "/tmp/b.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "history:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-missing".into(), ids);
        wait_idle(&queue, "hist-missing");

        let mut file_errors = 0usize;
        loop {
            let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            match topic.as_str() {
                "file:error" => file_errors += 1,
                "history:error" => {
                    panic!("missing store must warn to stderr and no-op, not emit history:error")
                }
                "batch:complete" => break,
                other => panic!("unexpected event: {other}"),
            }
        }
        assert_eq!(file_errors, 2);
    }

    #[tokio::test]
    async fn finish_batch_trims_old_history() {
        let _temp_guard = temp_guard();
        let root = tempfile::TempDir::new().unwrap();
        let out = root.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let app = make_app(
            root.path(),
            serde_json::json!({
                "outputFolder": out.to_str().unwrap(),
                "performance": { "adaptiveMemory": false, "memoryCeilingMb": 450 },
                "history": { "retentionDays": 10 }
            }),
        );
        let store = app.state::<crate::history::HistoryStore>();
        let now = now_ms();
        seed_session_start(&store, "old-11", now - 11 * 86_400_000, "/old");
        seed_session_start(&store, "old-30", now - 30 * 86_400_000, "/old");

        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-trim".into(), ids);
        wait_idle(&queue, "hist-trim");
        drain_until_complete(&rx);

        let fold = store.load().unwrap();
        let session_ids: Vec<&str> = fold.sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(session_ids, vec!["hist-trim"], "old sessions trimmed, fresh session kept");
    }

    #[tokio::test]
    async fn session_start_output_folder_matches_settings() {
        let _temp_guard = temp_guard();
        let (_root, app, out) = setup_app(450);
        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-folder".into(), ids.clone());
        wait_idle(&queue, "hist-folder");
        drain_until_complete(&rx);

        let store = app.state::<crate::history::HistoryStore>();
        let fold = store.load().unwrap();
        let s = &fold.sessions[0];
        assert_eq!(s.output_folder, out.to_str().unwrap());
        assert_eq!(s.file_count, ids.len());
    }

    #[tokio::test]
    async fn duration_ms_positive_for_processed_outcome() {
        let _temp_guard = temp_guard();
        let (_root, app, _out) = setup_app(450);
        let ids = add_entries(&app, &[
            ("a.jpg", crate::models::Category::Image, "/tmp/a.jpg"),
        ]);
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "hist-duration".into(), ids);
        wait_idle(&queue, "hist-duration");
        drain_until_complete(&rx);

        let store = app.state::<crate::history::HistoryStore>();
        let fold = store.load().unwrap();
        let s = &fold.sessions[0];
        assert_eq!(s.files.len(), 1);
        assert!(
            s.files[0].duration_ms > 0,
            "attempted job must carry a measured duration, got {}",
            s.files[0].duration_ms
        );
    }

    #[tokio::test]
    async fn panic_is_caught_and_reported_as_file_error() {
        let _temp_guard = temp_guard();
        let (_root, app, out) = setup_app(450);

        struct PanicProcessor;
        impl CompressionProcessor for PanicProcessor {
            fn category(&self) -> crate::models::Category {
                crate::models::Category::Image
            }
            fn supported_extensions(&self) -> &[&str] {
                &["jpg"]
            }
            fn compress_to_target(
                &self,
                _input: &std::path::Path,
                _output: &std::path::Path,
                _target_bytes: u64,
                _quality_floor: &crate::settings::QualityFloors,
                _cancel_token: CancellationToken,
                _progress: ProgressSender,
                _sidecar_paths: &crate::engine::SidecarPaths,
                _temp_dir: &std::path::Path,
            ) -> Result<CompressionResult, CompressionError> {
                panic!("simulated processor panic");
            }
        }

        // Exercise the same catch-unwind wrapper used by process_single_job
        // and assert it maps a panic to a file-class error.
        let result = run_processor_catch_panic(|| {
            PanicProcessor.compress_to_target(
                std::path::Path::new("/tmp/missing.jpg"),
                &out.join("panic.jpg"),
                1,
                &crate::settings::QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &crate::engine::SidecarPaths {
                    vips: Some("/bin/false".to_string()),
                    ffmpeg: None,
                    ffprobe: None,
                    gs: None,
                    qpdf: None,
                    gs_resources: None,
                },
                &out,
            )
        });
        match result {
            Err(CompressionError::OutputFileError(msg)) => {
                assert!(msg.contains("job panicked"), "got: {msg}");
            }
            other => panic!("expected OutputFileError, got {:?}", other),
        }

        // The batch itself continues: run a real batch with two failing files
        // and verify both emit file:error and batch:complete fires.
        let ids = add_entries(
            &app,
            &[
                ("a.jpg", crate::models::Category::Image, "/tmp/missing-a.jpg"),
                ("b.pdf", crate::models::Category::Pdf, "/tmp/missing-b.pdf"),
            ],
        );
        let rx = events_of(&app.handle(), &["file:error", "batch:complete"]);
        let queue = app.state::<QueueState<tauri::test::MockRuntime>>();
        QueueState::start_batch(queue.inner(), &app.handle(), "panic-batch".into(), ids);
        wait_idle(&queue, "panic-batch");

        let mut file_errors = 0usize;
        loop {
            let (topic, _) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            match topic.as_str() {
                "file:error" => file_errors += 1,
                "batch:complete" => break,
                other => panic!("unexpected event: {other}"),
            }
        }
        assert_eq!(file_errors, 2, "both files must error and the batch must continue");
    }
}
