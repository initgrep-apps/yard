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

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{LockResult, Mutex, MutexGuard};
use tauri::{AppHandle, Manager, Runtime};

use crate::policy::{Change, ConsentSource};

fn poison_recover<T>(lock: LockResult<MutexGuard<'_, T>>) -> MutexGuard<'_, T> {
    lock.unwrap_or_else(|e| {
        eprintln!("warn: history mutex was poisoned, recovering: {e}");
        e.into_inner()
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStartRecord {
    pub session_id: String,
    pub started_at: i64,
    pub file_count: usize,
    pub output_folder: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileResultRecord {
    pub session_id: String,
    pub file_id: String,
    pub completed_at: i64,
    pub name: String,
    pub path: String,
    pub output_path: Option<String>,
    pub category: String,
    pub target_bytes: u64,
    pub original_size: u64,
    pub compressed_size: Option<u64>,
    pub status: String,
    pub quality_used: Option<u32>,
    pub strategy_used: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
    #[serde(default)]
    pub changes: Vec<Change>,
    #[serde(default)]
    pub consent_source: Option<ConsentSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEndRecord {
    pub session_id: String,
    pub ended_at: i64,
    pub total_saved_bytes: u64,
    pub counts: Counts,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub done: u32,
    pub best_effort: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub unsupported: u32,
    pub already_under_target: u32,
    #[serde(default)]
    pub needs_permission: u32,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum HistoryRecordRef<'a> {
    SessionStart(&'a SessionStartRecord),
    File(&'a FileResultRecord),
    SessionEnd(&'a SessionEndRecord),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFile {
    pub name: String,
    pub path: String,
    pub output_path: Option<PathBuf>,
    pub category: String,
    pub target_bytes: u64,
    pub original_size: u64,
    pub compressed_size: Option<u64>,
    pub status: String,
    pub quality_used: Option<u32>,
    pub strategy_used: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub completed_at: i64,
    #[serde(default)]
    pub changes: Vec<Change>,
    #[serde(default)]
    pub consent_source: Option<ConsentSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySession {
    pub session_id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub output_folder: String,
    pub file_count: usize,
    pub files: Vec<HistoryFile>,
    pub outcome: Counts,
    pub total_saved_bytes: u64,
    pub cancelled: bool,
    pub interrupted: bool,
    pub matched_files: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoldResult {
    pub sessions: Vec<HistorySession>,
    pub skipped_lines: usize,
}

pub const DEFAULT_PAGE_SIZE: u32 = 50;
pub const MAX_PAGE_SIZE: u32 = 200;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryFilters {
    pub search: Option<String>,
    pub statuses: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
    pub since_ms: Option<i64>,
    pub output_folder: Option<String>,
    pub sort: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySummary {
    pub sessions: usize,
    pub files: usize,
    pub saved_bytes: u64,
    pub failed_files: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryResponse {
    pub sessions: Vec<HistorySession>,
    pub page: u32,
    pub page_size: u32,
    pub total_sessions: usize,
    pub total_files: usize,
    pub summary: HistorySummary,
    pub output_folders: Vec<String>,
    pub skipped_lines: usize,
}

pub(crate) fn clamp_page_size(n: u32) -> u32 {
    if n == 0 {
        DEFAULT_PAGE_SIZE
    } else {
        n.min(MAX_PAGE_SIZE)
    }
}

impl Default for HistoryResponse {
    fn default() -> Self {
        Self::empty(DEFAULT_PAGE_SIZE)
    }
}

impl HistoryResponse {
    pub fn empty(page_size: u32) -> Self {
        HistoryResponse {
            sessions: Vec::new(),
            page: 0,
            page_size: clamp_page_size(page_size),
            total_sessions: 0,
            total_files: 0,
            summary: HistorySummary::default(),
            output_folders: Vec::new(),
            skipped_lines: 0,
        }
    }
}

fn history_file_from(record: &FileResultRecord) -> HistoryFile {
    HistoryFile {
        name: record.name.clone(),
        path: record.path.clone(),
        output_path: record.output_path.as_ref().map(PathBuf::from),
        category: record.category.clone(),
        target_bytes: record.target_bytes,
        original_size: record.original_size,
        compressed_size: record.compressed_size,
        status: record.status.clone(),
        quality_used: record.quality_used,
        strategy_used: record.strategy_used.clone(),
        error: record.error.clone(),
        duration_ms: record.duration_ms,
        completed_at: record.completed_at,
        changes: record.changes.clone(),
        consent_source: record.consent_source,
    }
}

fn saved_for(file: &HistoryFile) -> u64 {
    file.compressed_size.map_or(0, |compressed| {
        if compressed > file.original_size {
            eprintln!(
                "info: history file {} output {compressed} larger than original {}, saved clamped to 0",
                file.path, file.original_size
            );
        }
        file.original_size.saturating_sub(compressed)
    })
}

fn outcome_from_files(session_id: &str, files: &[HistoryFile]) -> Counts {
    let mut counts = Counts::default();
    for file in files {
        match file.status.as_str() {
            "done" => counts.done += 1,
            "best_effort" => counts.best_effort += 1,
            "failed" => counts.failed += 1,
            "cancelled" => counts.cancelled += 1,
            "unsupported" => counts.unsupported += 1,
            "already_under_target" => counts.already_under_target += 1,
            "needs_permission" => counts.needs_permission += 1,
            other => eprintln!(
                "warn: history fold session {session_id} file {} has unknown status: {other}",
                file.path
            ),
        }
    }
    counts
}

fn parse_or_skip<T: serde::de::DeserializeOwned>(
    line: &str,
    kind: &str,
    entry_index: usize,
) -> Option<T> {
    match serde_json::from_str(line) {
        Ok(parsed) => Some(parsed),
        Err(e) => {
            eprintln!(
                "warn: history fold skipped invalid {kind} record at entry {entry_index}: {e}"
            );
            None
        }
    }
}

pub fn fold_records(lines: &[String]) -> FoldResult {
    let mut sessions: Vec<HistorySession> = Vec::new();
    let mut session_index: HashMap<String, usize> = HashMap::new();
    let mut file_indexes: Vec<HashMap<String, usize>> = Vec::new();
    let mut skipped_lines = 0usize;

    for (entry, line) in lines.iter().enumerate() {
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(e) => {
                eprintln!("warn: history fold skipped malformed entry {entry}: {e}");
                skipped_lines += 1;
                continue;
            }
        };
        let kind = match value.get("type").and_then(|t| t.as_str()) {
            Some(kind) => kind,
            None => {
                eprintln!("warn: history fold skipped entry {entry} without a string \"type\"");
                skipped_lines += 1;
                continue;
            }
        };
        match kind {
            "session_start" => {
                let Some(record) =
                    parse_or_skip::<SessionStartRecord>(line, "session_start", entry)
                else {
                    skipped_lines += 1;
                    continue;
                };
                match session_index.get(&record.session_id).copied() {
                    Some(index) => {
                        let session = &mut sessions[index];
                        session.started_at = record.started_at;
                        session.output_folder = record.output_folder;
                        session.file_count = record.file_count;
                    }
                    None => {
                        session_index.insert(record.session_id.clone(), sessions.len());
                        file_indexes.push(HashMap::new());
                        sessions.push(HistorySession {
                            session_id: record.session_id,
                            started_at: record.started_at,
                            ended_at: None,
                            output_folder: record.output_folder,
                            file_count: record.file_count,
                            files: Vec::new(),
                            outcome: Counts::default(),
                            total_saved_bytes: 0,
                            cancelled: false,
                            interrupted: false,
                            matched_files: None,
                        });
                    }
                }
            }
            "file" => {
                let Some(record) = parse_or_skip::<FileResultRecord>(line, "file", entry) else {
                    skipped_lines += 1;
                    continue;
                };
                let Some(index) = session_index.get(&record.session_id).copied() else {
                    eprintln!(
                        "warn: history fold skipped file record at entry {entry} for unknown session: {}",
                        record.session_id
                    );
                    skipped_lines += 1;
                    continue;
                };
                if sessions[index].ended_at.is_some() {
                    eprintln!(
                        "warn: history fold skipped file record at entry {entry} after session_end: session {} file {}",
                        record.session_id, record.file_id
                    );
                    skipped_lines += 1;
                    continue;
                }
                let history_file = history_file_from(&record);
                match file_indexes[index].get(&record.file_id).copied() {
                    Some(file_index) => sessions[index].files[file_index] = history_file,
                    None => {
                        let position = sessions[index].files.len();
                        file_indexes[index].insert(record.file_id, position);
                        sessions[index].files.push(history_file);
                    }
                }
            }
            "session_end" => {
                let Some(record) = parse_or_skip::<SessionEndRecord>(line, "session_end", entry)
                else {
                    skipped_lines += 1;
                    continue;
                };
                let Some(index) = session_index.get(&record.session_id).copied() else {
                    eprintln!(
                        "warn: history fold skipped session_end at entry {entry} for unknown session: {}",
                        record.session_id
                    );
                    skipped_lines += 1;
                    continue;
                };
                let session = &mut sessions[index];
                if session.ended_at.is_some() {
                    eprintln!(
                        "warn: history fold skipped duplicate session_end at entry {entry}: {}",
                        record.session_id
                    );
                    skipped_lines += 1;
                    continue;
                }
                session.ended_at = Some(record.ended_at);
                session.total_saved_bytes = record.total_saved_bytes;
                session.cancelled = record.cancelled;
                session.outcome = record.counts;
            }
            _ => {}
        }
    }

    for session in &mut sessions {
        if session.ended_at.is_none() {
            session.interrupted = true;
            session.outcome = outcome_from_files(&session.session_id, &session.files);
            session.total_saved_bytes = session
                .files
                .iter()
                .map(saved_for)
                .fold(0u64, u64::saturating_add);
        }
        session.files.sort_by_key(|file| file.completed_at);
    }
    sessions.sort_by_key(|session| std::cmp::Reverse(session.started_at));

    FoldResult {
        sessions,
        skipped_lines,
    }
}

fn matches_search(text: &str, needle: &str) -> bool {
    text.to_lowercase().contains(needle)
}

fn file_hit(file: &HistoryFile, needle: &str) -> bool {
    matches_search(&file.name, needle)
        || matches_search(&file.path, needle)
        || file
            .output_path
            .as_ref()
            .is_some_and(|path| matches_search(&path.to_string_lossy(), needle))
        || file
            .error
            .as_deref()
            .is_some_and(|error| matches_search(error, needle))
}

fn normalize_text(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn normalize_list(values: Option<&[String]>) -> Option<Vec<String>> {
    let values: Vec<String> = values?
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn file_matches(
    file: &HistoryFile,
    statuses: Option<&[String]>,
    categories: Option<&[String]>,
    needle: Option<&str>,
) -> bool {
    statuses.is_none_or(|statuses| statuses.contains(&file.status))
        && categories.is_none_or(|categories| categories.contains(&file.category))
        && needle.is_none_or(|needle| file_hit(file, needle))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum HistorySort {
    Newest,
    Oldest,
}

fn effective_sort(raw: Option<&str>) -> HistorySort {
    match normalize_text(raw) {
        Some("oldest") => HistorySort::Oldest,
        Some("newest") | None => HistorySort::Newest,
        Some(other) => {
            eprintln!("warn: history query unknown sort {other:?}, falling back to newest");
            HistorySort::Newest
        }
    }
}

pub fn query_history(
    fold: FoldResult,
    filters: &HistoryFilters,
    page: u32,
    page_size: u32,
) -> HistoryResponse {
    debug_assert!(
        page_size >= 1 && page_size <= MAX_PAGE_SIZE,
        "query_history page_size {page_size} outside 1..={MAX_PAGE_SIZE}; the command layer must clamp before calling"
    );
    let output_folders: Vec<String> = fold
        .sessions
        .iter()
        .filter_map(|session| normalize_text(Some(session.output_folder.as_str())))
        .collect::<BTreeSet<&str>>()
        .into_iter()
        .map(str::to_string)
        .collect();

    let needle = normalize_text(filters.search.as_deref()).map(str::to_lowercase);
    let statuses = normalize_list(filters.statuses.as_deref());
    let categories = normalize_list(filters.categories.as_deref());
    let output_folder = normalize_text(filters.output_folder.as_deref());
    let since_ms = filters.since_ms;

    let mut visible: Vec<HistorySession> = Vec::new();
    for mut session in fold.sessions {
        if since_ms.is_some_and(|since_ms| session.started_at < since_ms) {
            continue;
        }
        let session_folder = normalize_text(Some(session.output_folder.as_str()));
        if output_folder.is_some_and(|folder| session_folder != Some(folder)) {
            continue;
        }

        let mut file_search_hit = false;
        if let Some(needle) = needle.as_deref() {
            let folder_hit = matches_search(&session.output_folder, needle);
            file_search_hit = session.files.iter().any(|file| file_hit(file, needle));
            if !folder_hit && !file_search_hit {
                continue;
            }
        }

        let file_constraints_active = statuses.is_some() || categories.is_some();
        let needle_for_files = needle.as_deref().filter(|_| file_search_hit);
        if file_constraints_active || needle_for_files.is_some() {
            session.files.retain(|file| {
                file_matches(
                    file,
                    statuses.as_deref(),
                    categories.as_deref(),
                    needle_for_files,
                )
            });
            if session.files.is_empty() {
                continue;
            }
            session.matched_files = Some(session.files.len());
        }
        visible.push(session);
    }

    match effective_sort(filters.sort.as_deref()) {
        HistorySort::Oldest => visible.sort_by_key(|session| session.started_at),
        HistorySort::Newest => visible.sort_by_key(|session| std::cmp::Reverse(session.started_at)),
    }

    let total_sessions = visible.len();
    let mut total_files = 0usize;
    let mut saved_bytes = 0u64;
    let mut failed_files = 0usize;
    for session in &visible {
        for file in &session.files {
            total_files += 1;
            saved_bytes = saved_bytes.saturating_add(saved_for(file));
            if file.status == "failed" {
                failed_files += 1;
            }
        }
    }

    let page_size = page_size.max(1);
    let last_page = total_sessions
        .div_ceil(page_size as usize)
        .saturating_sub(1)
        .min(u32::MAX as usize) as u32;
    let page = page.min(last_page);
    let start = (page as usize).saturating_mul(page_size as usize);
    let sessions = visible
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();

    let summary = HistorySummary {
        sessions: total_sessions,
        files: total_files,
        saved_bytes,
        failed_files,
    };
    debug_assert_eq!(total_sessions, summary.sessions);
    debug_assert_eq!(total_files, summary.files);

    HistoryResponse {
        sessions,
        page,
        page_size,
        total_sessions,
        total_files,
        summary,
        output_folders,
        skipped_lines: fold.skipped_lines,
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LineRecord {
    SessionStart(SessionStartRecord),
    File(FileResultRecord),
    SessionEnd(SessionEndRecord),
}

fn parse_line_ref(line: &str) -> Option<(String, i64, bool)> {
    match serde_json::from_str::<LineRecord>(line) {
        Ok(LineRecord::SessionStart(record)) => Some((record.session_id, record.started_at, true)),
        Ok(LineRecord::File(record)) => Some((record.session_id, record.completed_at, false)),
        Ok(LineRecord::SessionEnd(record)) => Some((record.session_id, record.ended_at, false)),
        Err(_) => None,
    }
}

pub fn split_by_retention(
    lines: Vec<String>,
    retention_days: u32,
    now_ms: i64,
) -> (Vec<String>, usize) {
    let retention_days = if retention_days == 0 {
        90
    } else {
        retention_days
    };
    let cutoff = now_ms.saturating_sub(i64::from(retention_days) * 86_400_000);
    let mut newest_by_session: HashMap<String, i64> = HashMap::new();
    let mut bound: Vec<Option<String>> = Vec::with_capacity(lines.len());

    for line in &lines {
        let Some((session_id, at, is_start)) = parse_line_ref(line) else {
            bound.push(None);
            continue;
        };
        if !is_start && !newest_by_session.contains_key(&session_id) {
            bound.push(None);
            continue;
        }
        let newest = newest_by_session.entry(session_id.clone()).or_insert(at);
        *newest = (*newest).max(at);
        bound.push(Some(session_id));
    }

    let drop: HashSet<String> = newest_by_session
        .into_iter()
        .filter(|(_, newest)| *newest < cutoff)
        .map(|(session_id, _)| session_id)
        .collect();

    let mut kept = Vec::with_capacity(lines.len());
    for (line, session_id) in lines.into_iter().zip(bound) {
        if session_id.is_none_or(|session_id| !drop.contains(&session_id)) {
            kept.push(line);
        }
    }

    (kept, drop.len())
}

fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub struct HistoryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl HistoryStore {
    pub fn new_in(dir: &Path) -> Self {
        HistoryStore {
            path: dir.join("history.jsonl"),
            lock: Mutex::new(()),
        }
    }

    pub fn new<R: Runtime>(app: &AppHandle<R>) -> Result<HistoryStore, String> {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("history store: app data dir unavailable: {e}"))?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("history store: create data dir {}: {e}", dir.display()))?;
        Ok(HistoryStore::new_in(&dir))
    }

    pub(crate) fn file_path(&self) -> &Path {
        &self.path
    }

    pub fn append_session_start(&self, record: &SessionStartRecord) -> Result<(), String> {
        self.append(HistoryRecordRef::SessionStart(record))
    }

    pub fn append_file(&self, record: &FileResultRecord) -> Result<(), String> {
        self.append(HistoryRecordRef::File(record))
    }

    pub fn append_session_end(&self, record: &SessionEndRecord) -> Result<(), String> {
        self.append(HistoryRecordRef::SessionEnd(record))
    }

    pub(crate) fn read_lines(&self) -> Result<Vec<String>, String> {
        let _guard = poison_recover(self.lock.lock());
        self.read_lines_unlocked()
    }

    fn read_lines_unlocked(&self) -> Result<Vec<String>, String> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(format!(
                    "history read failed: read {}: {e}",
                    self.path.display()
                ))
            }
        };
        let path = self.path.display();
        Ok(bytes
            .split(|b| *b == b'\n')
            .enumerate()
            .map(|(i, line)| {
                if let Err(e) = std::str::from_utf8(line) {
                    eprintln!(
                        "warn: history line {i} had invalid UTF-8, lossy-decoded ({path}): {e}"
                    );
                }
                String::from_utf8_lossy(line).trim().to_string()
            })
            .filter(|line| !line.is_empty())
            .collect())
    }

    fn tmp_path(&self) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".tmp");
        self.path.with_file_name(name)
    }

    pub fn trim(&self, retention_days: u32, now_ms: i64) -> Result<u32, String> {
        let _guard = poison_recover(self.lock.lock());
        let lines = self.read_lines_unlocked()?;
        let (kept, dropped) = split_by_retention(lines, retention_days, now_ms);
        let tmp = self.tmp_path();
        remove_if_exists(&tmp).map_err(|e| {
            format!(
                "history write failed: remove stale temp {}: {e}",
                tmp.display()
            )
        })?;
        if dropped == 0 {
            return Ok(0);
        }
        let mut contents = String::new();
        for line in &kept {
            contents.push_str(line);
            contents.push('\n');
        }
        let mut file = std::fs::File::create(&tmp)
            .map_err(|e| format!("history write failed: create {}: {e}", tmp.display()))?;
        write_terminated(&mut file, contents.as_bytes(), &tmp)?;
        file.sync_all()
            .map_err(|e| format!("history write failed: sync {}: {e}", tmp.display()))?;
        drop(file);
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            format!(
                "history write failed: rename {} to {}: {e}",
                tmp.display(),
                self.path.display()
            )
        })?;
        Ok(u32::try_from(dropped).unwrap_or(u32::MAX))
    }

    pub fn clear(&self) -> Result<(), String> {
        let _guard = poison_recover(self.lock.lock());
        let tmp = self.tmp_path();
        remove_if_exists(&tmp)
            .map_err(|e| format!("history clear failed: remove {}: {e}", tmp.display()))?;
        remove_if_exists(&self.path)
            .map_err(|e| format!("history clear failed: remove {}: {e}", self.path.display()))?;
        Ok(())
    }

    pub fn load(&self) -> Result<FoldResult, String> {
        Ok(fold_records(&self.read_lines()?))
    }

    fn append(&self, record: HistoryRecordRef<'_>) -> Result<(), String> {
        let _guard = poison_recover(self.lock.lock());
        let path = self.path.display();
        let line = serde_json::to_string(&record)
            .map_err(|e| format!("history write failed: serialize record for {path}: {e}"))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "history write failed: create parent dir {} for {path}: {e}",
                    parent.display()
                )
            })?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&self.path)
            .map_err(|e| format!("history write failed: open {path}: {e}"))?;
        let len = file
            .seek(SeekFrom::End(0))
            .map_err(|e| format!("history write failed: seek {path}: {e}"))?;
        let mut buf = Vec::with_capacity(line.len() + 2);
        if len > 0 {
            let mut last = [0u8; 1];
            let ends_with_newline = match file
                .seek(SeekFrom::End(-1))
                .and_then(|_| file.read_exact(&mut last))
            {
                Ok(()) => last[0] == b'\n',
                Err(e) => {
                    eprintln!(
                        "warn: history append newline probe {path} failed, inserting separator: {e}"
                    );
                    false
                }
            };
            if !ends_with_newline {
                buf.push(b'\n');
            }
        }
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
        write_terminated(&mut file, &buf, &self.path)
    }
}

fn write_terminated(file: &mut std::fs::File, buf: &[u8], path: &Path) -> Result<(), String> {
    file.write_all(buf)
        .map_err(|e| format!("history write failed: write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn small_end(session_id: &str) -> SessionEndRecord {
        big_end(session_id.into())
    }

    fn big_end(session_id: String) -> SessionEndRecord {
        SessionEndRecord {
            session_id,
            ended_at: 0,
            total_saved_bytes: 0,
            counts: Counts::default(),
            cancelled: false,
        }
    }

    fn old_start() -> SessionStartRecord {
        SessionStartRecord {
            session_id: "old".into(),
            started_at: 0,
            file_count: 1,
            output_folder: "/o".into(),
        }
    }

    fn sorted_keys(value: &serde_json::Value) -> Vec<String> {
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    }

    fn line(value: serde_json::Value) -> String {
        value.to_string()
    }

    fn start(sid: &str, at: i64) -> serde_json::Value {
        serde_json::json!({
            "type": "session_start", "sessionId": sid, "startedAt": at,
            "fileCount": 1, "outputFolder": "/out"
        })
    }

    fn file(
        sid: &str,
        fid: &str,
        at: i64,
        status: &str,
        sizes: (u64, Option<u64>),
    ) -> serde_json::Value {
        serde_json::json!({
            "type": "file", "sessionId": sid, "fileId": fid, "completedAt": at,
            "name": "a.jpg", "path": "/in/a.jpg", "outputPath": "/out/a.jpg",
            "category": "image", "targetBytes": 1024,
            "originalSize": sizes.0, "compressedSize": sizes.1,
            "status": status, "qualityUsed": 70, "strategyUsed": "quality=70",
            "error": null, "durationMs": 12
        })
    }

    fn end(sid: &str, at: i64) -> serde_json::Value {
        serde_json::json!({
            "type": "session_end", "sessionId": sid, "endedAt": at,
            "totalSavedBytes": 100,
            "counts": { "done": 1, "bestEffort": 0, "failed": 0, "cancelled": 0,
                        "unsupported": 0, "alreadyUnderTarget": 0 },
            "cancelled": false
        })
    }

    #[test]
    fn append_session_start_writes_camel_case_line() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store
            .append_session_start(&SessionStartRecord {
                session_id: "s1".into(),
                started_at: 1_760_000_000_000,
                file_count: 3,
                output_folder: "/out".into(),
            })
            .unwrap();
        let line = store.read_lines().unwrap().pop().unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "session_start");
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["startedAt"], 1_760_000_000_000i64);
        assert_eq!(v["fileCount"], 3);
        assert_eq!(v["outputFolder"], "/out");
    }

    #[test]
    fn append_file_serializes_optional_fields_as_null() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store
            .append_file(&FileResultRecord {
                session_id: "s1".into(),
                file_id: "f1".into(),
                completed_at: 1_760_000_001_000,
                name: "a.jpg".into(),
                path: "/in/a.jpg".into(),
                output_path: None,
                category: "image".into(),
                target_bytes: 204_800,
                original_size: 1024,
                compressed_size: None,
                status: "cancelled".into(),
                quality_used: None,
                strategy_used: None,
                error: None,
                duration_ms: 0,
                changes: Vec::new(),
                consent_source: None,
            })
            .unwrap();
        let line = store.read_lines().unwrap().pop().unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "file");
        assert!(v["outputPath"].is_null());
        assert!(v["compressedSize"].is_null());
        assert!(v["qualityUsed"].is_null());
        assert!(v["strategyUsed"].is_null());
        assert!(v["error"].is_null());
        assert_eq!(v["status"], "cancelled");
    }

    #[test]
    fn append_session_end_writes_counts_and_cancelled() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store
            .append_session_end(&SessionEndRecord {
                session_id: "s1".into(),
                ended_at: 1_760_000_010_000,
                total_saved_bytes: 4096,
                counts: Counts {
                    done: 2,
                    best_effort: 0,
                    failed: 1,
                    cancelled: 1,
                    unsupported: 0,
                    already_under_target: 0,
                    needs_permission: 0,
                },
                cancelled: true,
            })
            .unwrap();
        let line = store.read_lines().unwrap().pop().unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "session_end");
        assert_eq!(v["counts"]["bestEffort"], 0);
        assert_eq!(v["counts"]["alreadyUnderTarget"], 0);
        assert_eq!(v["cancelled"], true);
    }

    #[test]
    fn read_lines_missing_file_returns_empty() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        assert!(store.read_lines().unwrap().is_empty());
    }

    #[test]
    fn read_lines_skips_blank_lines() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        std::fs::write(store.file_path(), "{\"a\":1}\n\n{\"b\":2}\n").unwrap();
        let lines = store.read_lines().unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "{\"a\":1}");
        assert_eq!(lines[1], "{\"b\":2}");
    }

    #[test]
    fn read_lines_trims_whitespace_around_lines() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let start_line = line(start("s1", 10));
        std::fs::write(store.file_path(), format!("  \n {start_line} \r\n")).unwrap();

        let lines = store.read_lines().unwrap();
        assert_eq!(lines, vec![start_line.clone()], "only padded JSON survives");

        let out = store.load().unwrap();
        assert_eq!(out.skipped_lines, 0, "padded record parses after trimming");
        assert_eq!(out.sessions.len(), 1);
        assert_eq!(out.sessions[0].session_id, "s1");
    }

    #[test]
    fn read_lines_io_error_returns_err() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        std::fs::create_dir(store.file_path()).unwrap();
        let err = store.read_lines().unwrap_err();
        assert!(err.starts_with("history read failed: read "), "{err}");
        assert!(
            err.contains(&store.file_path().display().to_string()),
            "{err}"
        );
    }

    #[test]
    fn append_creates_parent_directories() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(&dir.path().join("a").join("b"));
        store.append_session_end(&small_end("s1")).unwrap();
        assert!(store.file_path().exists());
        assert_eq!(
            store.file_path(),
            dir.path().join("a").join("b").join("history.jsonl")
        );
    }

    #[test]
    fn append_propagates_write_error() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        std::fs::create_dir(store.file_path()).unwrap();
        let err = store.append_session_end(&small_end("s1")).unwrap_err();
        assert!(err.starts_with("history write failed:"), "{err}");
        assert!(
            err.contains(&store.file_path().display().to_string()),
            "{err}"
        );
    }

    #[test]
    fn append_parent_is_file_surfaces_create_stage_error() {
        let dir = tempdir().unwrap();
        let blocker = dir.path().join("blocker.txt");
        std::fs::write(&blocker, b"x").unwrap();
        let store = HistoryStore::new_in(&blocker.join("child"));
        let err = store.append_session_end(&small_end("s1")).unwrap_err();
        assert!(
            err.starts_with("history write failed: create parent dir "),
            "{err}"
        );
        assert!(
            err.contains(&store.file_path().display().to_string()),
            "{err}"
        );
    }

    #[test]
    fn write_failure_surfaces_write_stage_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        std::fs::write(&path, "").unwrap();
        let mut file = std::fs::File::open(&path).unwrap();
        let err = write_terminated(&mut file, b"{\"a\":1}\n", &path).unwrap_err();
        assert!(err.starts_with("history write failed: write "), "{err}");
    }

    #[test]
    fn append_repairs_missing_trailing_newline() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let partial = b"{\"type\":\"session_end\",\"sessionId\":\"torn";
        std::fs::write(store.file_path(), partial).unwrap();

        store.append_session_end(&small_end("s2")).unwrap();

        let lines = store.read_lines().unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], String::from_utf8_lossy(partial));
        let parsed: SessionEndRecord = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(parsed.session_id, "s2");

        let mut expected = partial.to_vec();
        expected.push(b'\n');
        expected.extend_from_slice(lines[1].as_bytes());
        expected.push(b'\n');
        assert_eq!(std::fs::read(store.file_path()).unwrap(), expected);
    }

    #[test]
    fn append_writes_single_terminated_line() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());

        store.append_session_end(&small_end("s1")).unwrap();
        let raw = std::fs::read(store.file_path()).unwrap();
        assert_eq!(raw.iter().rev().take_while(|b| **b == b'\n').count(), 1);
        assert_ne!(raw[0], b'\n');

        store.append_session_end(&small_end("s2")).unwrap();
        let raw = std::fs::read(store.file_path()).unwrap();
        assert_eq!(raw.iter().filter(|b| **b == b'\n').count(), 2);
        assert!(!raw.ends_with(b"\n\n"));
        assert_eq!(store.read_lines().unwrap().len(), 2);
    }

    #[test]
    fn read_lines_tolerates_invalid_utf8_bytes() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"{\"a\":1}\n");
        bytes.extend_from_slice(&[0xff, 0xfe, 0x80]);
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(b"{\"b\":2}\n");
        std::fs::write(store.file_path(), bytes).unwrap();

        let lines = store.read_lines().unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "{\"a\":1}");
        assert!(!lines[1].is_empty());
        assert_eq!(lines[2], "{\"b\":2}");
    }

    #[test]
    fn append_creates_file_and_appends_in_order() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        assert!(!store.file_path().exists());
        store.append_session_end(&small_end("s1")).unwrap();
        store.append_session_end(&small_end("s2")).unwrap();
        let lines = store.read_lines().unwrap();
        assert_eq!(lines.len(), 2);
        let first: SessionEndRecord = serde_json::from_str(&lines[0]).unwrap();
        let second: SessionEndRecord = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(first.session_id, "s1");
        assert_eq!(second.session_id, "s2");
    }

    #[test]
    fn append_session_end_roundtrips() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let record = SessionEndRecord {
            session_id: "s1".into(),
            ended_at: 42,
            total_saved_bytes: 4096,
            counts: Counts {
                done: 2,
                best_effort: 1,
                failed: 1,
                cancelled: 0,
                unsupported: 0,
                already_under_target: 3,
                needs_permission: 0,
            },
            cancelled: true,
        };
        store.append_session_end(&record).unwrap();
        let line = store.read_lines().unwrap().pop().unwrap();
        let parsed: SessionEndRecord = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed, record);
    }

    #[test]
    fn concurrent_appends_keep_lines_intact() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let store = &store;
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for t in 0..2 {
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    for i in 0..25 {
                        store
                            .append_session_end(&big_end(format!("t{t}-{i}-{}", "x".repeat(8192))))
                            .unwrap();
                    }
                });
            }
        });
        let lines = store.read_lines().unwrap();
        assert_eq!(lines.len(), 50);
        let mut ids = std::collections::HashSet::new();
        for line in &lines {
            let parsed: SessionEndRecord = serde_json::from_str(line).unwrap();
            ids.insert(parsed.session_id);
        }
        for t in 0..2 {
            for i in 0..25 {
                let expected = format!("t{t}-{i}-{}", "x".repeat(8192));
                assert!(ids.contains(&expected), "missing {t}-{i}");
            }
        }
        assert_eq!(ids.len(), 50);
    }

    struct StopGuard<'a>(&'a std::sync::atomic::AtomicBool);

    impl Drop for StopGuard<'_> {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn read_during_write_never_sees_torn_line() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let stop = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let _guard = StopGuard(&stop);
                for i in 0..100 {
                    store
                        .append_session_end(&big_end(format!("w-{i}-{}", "y".repeat(8192))))
                        .unwrap();
                }
            });
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                for line in store.read_lines().unwrap() {
                    serde_json::from_str::<serde_json::Value>(&line).unwrap();
                }
            }
        });
        for line in store.read_lines().unwrap() {
            serde_json::from_str::<serde_json::Value>(&line).unwrap();
        }
    }

    #[test]
    fn append_blocks_until_lock_released() {
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let store = &store;
        let guard = store.lock.lock().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let handle = scope.spawn(move || {
                started_tx.send(()).unwrap();
                store.append_session_end(&small_end("s1")).unwrap();
                done_tx.send(()).unwrap();
            });
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("worker thread never reached append");
            assert!(
                done_rx.recv_timeout(Duration::from_millis(150)).is_err(),
                "append completed while the store lock was held"
            );
            drop(guard);
            assert!(done_rx.recv_timeout(Duration::from_secs(5)).is_ok());
            handle.join().unwrap();
        });
        let lines = store.read_lines().unwrap();
        assert_eq!(lines.len(), 1);
        let parsed: SessionEndRecord = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(parsed.session_id, "s1");
    }

    #[test]
    fn read_lines_blocks_until_lock_released() {
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        std::fs::write(store.file_path(), "{\"a\":1}\n").unwrap();
        let store = &store;
        let guard = store.lock.lock().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let handle = scope.spawn(move || {
                started_tx.send(()).unwrap();
                let lines = store.read_lines().unwrap();
                done_tx.send(lines).unwrap();
            });
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("worker thread never reached read_lines");
            assert!(
                done_rx.recv_timeout(Duration::from_millis(150)).is_err(),
                "read_lines completed while the store lock was held"
            );
            drop(guard);
            let lines = done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(lines, vec!["{\"a\":1}".to_string()]);
            handle.join().unwrap();
        });
    }

    #[test]
    fn trim_blocks_until_lock_released() {
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store.append_session_start(&old_start()).unwrap();
        let store = &store;
        let guard = store.lock.lock().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let handle = scope.spawn(move || {
                started_tx.send(()).unwrap();
                let dropped = store.trim(1, 100 * DAY_MS).unwrap();
                done_tx.send(dropped).unwrap();
            });
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("worker thread never reached trim");
            assert!(
                done_rx.recv_timeout(Duration::from_millis(150)).is_err(),
                "trim completed while the store lock was held"
            );
            drop(guard);
            let dropped = done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(dropped, 1);
            handle.join().unwrap();
        });
        assert!(store.read_lines().unwrap().is_empty());
    }

    #[test]
    fn clear_blocks_until_lock_released() {
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store.append_session_end(&small_end("s1")).unwrap();
        let store = &store;
        let guard = store.lock.lock().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let handle = scope.spawn(move || {
                started_tx.send(()).unwrap();
                store.clear().unwrap();
                done_tx.send(()).unwrap();
            });
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("worker thread never reached clear");
            assert!(
                done_rx.recv_timeout(Duration::from_millis(150)).is_err(),
                "clear completed while the store lock was held"
            );
            drop(guard);
            assert!(done_rx.recv_timeout(Duration::from_secs(5)).is_ok());
            handle.join().unwrap();
        });
        assert!(!store.file_path().exists());
    }

    #[test]
    fn record_schema_keys_are_pinned() {
        let start = serde_json::to_value(HistoryRecordRef::SessionStart(&SessionStartRecord {
            session_id: "s1".into(),
            started_at: 1,
            file_count: 0,
            output_folder: "/out".into(),
        }))
        .unwrap();
        assert_eq!(
            sorted_keys(&start),
            vec![
                "fileCount",
                "outputFolder",
                "sessionId",
                "startedAt",
                "type",
            ]
        );

        let file = serde_json::to_value(HistoryRecordRef::File(&FileResultRecord {
            session_id: "s1".into(),
            file_id: "f1".into(),
            completed_at: 1,
            name: "a.jpg".into(),
            path: "/in/a.jpg".into(),
            output_path: None,
            category: "image".into(),
            target_bytes: 1,
            original_size: 1,
            compressed_size: None,
            status: "done".into(),
            quality_used: None,
            strategy_used: None,
            error: None,
            duration_ms: 0,
            changes: Vec::new(),
            consent_source: None,
        }))
        .unwrap();
        assert_eq!(
            sorted_keys(&file),
            vec![
                "category",
                "changes",
                "completedAt",
                "compressedSize",
                "consentSource",
                "durationMs",
                "error",
                "fileId",
                "name",
                "originalSize",
                "outputPath",
                "path",
                "qualityUsed",
                "sessionId",
                "status",
                "strategyUsed",
                "targetBytes",
                "type",
            ]
        );

        let end = serde_json::to_value(HistoryRecordRef::SessionEnd(&small_end("s1"))).unwrap();
        assert_eq!(
            sorted_keys(&end),
            vec![
                "cancelled",
                "counts",
                "endedAt",
                "sessionId",
                "totalSavedBytes",
                "type",
            ]
        );
        assert_eq!(
            sorted_keys(&end["counts"]),
            vec![
                "alreadyUnderTarget",
                "bestEffort",
                "cancelled",
                "done",
                "failed",
                "needsPermission",
                "unsupported",
            ]
        );
    }

    #[test]
    fn append_and_read_roundtrip_unicode_paths() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let file = FileResultRecord {
            session_id: "séance-漢字-😀".into(),
            file_id: "f1".into(),
            completed_at: 1,
            name: "café-漢字-😀.jpg".into(),
            path: "/in/café/漢字/😀.jpg".into(),
            output_path: Some("/out/café/漢字/😀.jpg".into()),
            category: "image".into(),
            target_bytes: 1,
            original_size: 1,
            compressed_size: None,
            status: "failed".into(),
            quality_used: None,
            strategy_used: None,
            error: Some("échec: 漢字 😀".into()),
            duration_ms: 0,
            changes: Vec::new(),
            consent_source: None,
        };
        store.append_file(&file).unwrap();
        let line = store.read_lines().unwrap().pop().unwrap();
        let parsed: FileResultRecord = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed, file);
    }

    #[test]
    fn fold_single_complete_session() {
        let lines = vec![
            line(start("s1", 100)),
            line(file("s1", "f1", 110, "done", (500, Some(100)))),
            line(end("s1", 120)),
        ];
        let out = fold_records(&lines);
        assert_eq!(out.skipped_lines, 0);
        assert_eq!(out.sessions.len(), 1);
        let s = &out.sessions[0];
        assert_eq!(s.session_id, "s1");
        assert_eq!(s.started_at, 100);
        assert_eq!(s.ended_at, Some(120));
        assert_eq!(s.output_folder, "/out");
        assert!(!s.cancelled);
        assert!(!s.interrupted);
        assert_eq!(s.total_saved_bytes, 100);
        assert_eq!(s.outcome.done, 1);
        assert_eq!(s.file_count, 1);
        assert_eq!(s.matched_files, None);
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].quality_used, Some(70));
        assert_eq!(s.files[0].path, "/in/a.jpg");
        assert_eq!(s.files[0].target_bytes, 1024);
        assert_eq!(s.files[0].output_path, Some(PathBuf::from("/out/a.jpg")));
    }

    #[test]
    fn fold_file_category_passthrough() {
        let mut file_record = file("s1", "f1", 110, "done", (500, Some(100)));
        file_record["category"] = serde_json::json!("video");
        let lines = vec![
            line(start("s1", 100)),
            line(file_record),
            line(end("s1", 120)),
        ];
        let out = fold_records(&lines);
        assert_eq!(out.sessions[0].files[0].category, "video");
    }

    #[test]
    fn fold_file_error_text_passthrough() {
        let mut file_record = file("s1", "f1", 110, "failed", (500, None));
        file_record["error"] = serde_json::json!("boom");
        let lines = vec![line(start("s1", 100)), line(file_record)];
        let out = fold_records(&lines);
        assert_eq!(
            out.sessions[0].files[0].error,
            Some("boom".to_string())
        );
    }

    #[test]
    fn fold_uses_session_end_values_verbatim() {
        let mut end_record = end("s1", 120);
        end_record["counts"] = serde_json::json!({
            "done": 0, "bestEffort": 2, "failed": 0,
            "cancelled": 0, "unsupported": 0, "alreadyUnderTarget": 0
        });
        let lines = vec![
            line(start("s1", 100)),
            line(file("s1", "f1", 110, "done", (500, Some(100)))),
            line(end_record),
        ];
        let out = fold_records(&lines);
        let s = &out.sessions[0];
        assert_eq!(
            s.total_saved_bytes, 100,
            "end totalSavedBytes wins over file-derived 400"
        );
        assert_eq!(
            s.outcome.best_effort, 2,
            "counts come verbatim from the end record"
        );
        assert_eq!(s.outcome.done, 0);
    }

    #[test]
    fn fold_missing_session_end_is_interrupted() {
        let lines = vec![
            line(start("s1", 10)),
            line(file("s1", "f1", 11, "done", (5, Some(1)))),
        ];
        let out = fold_records(&lines);
        assert!(out.sessions[0].interrupted);
        assert!(!out.sessions[0].cancelled);
        assert_eq!(out.sessions[0].ended_at, None);
        assert_eq!(out.sessions[0].total_saved_bytes, 4);
    }

    #[test]
    fn fold_interrupted_computes_all_six_status_counts() {
        let lines = vec![
            line(start("s1", 10)),
            line(file("s1", "f1", 11, "done", (10, Some(1)))),
            line(file("s1", "f2", 12, "best_effort", (10, Some(2)))),
            line(file("s1", "f3", 13, "failed", (10, None))),
            line(file("s1", "f4", 14, "cancelled", (10, None))),
            line(file("s1", "f5", 15, "unsupported", (10, None))),
            line(file("s1", "f6", 16, "already_under_target", (10, None))),
        ];
        let out = fold_records(&lines);
        let s = &out.sessions[0];
        assert!(s.interrupted);
        assert_eq!(s.outcome.done, 1);
        assert_eq!(s.outcome.best_effort, 1);
        assert_eq!(s.outcome.failed, 1);
        assert_eq!(s.outcome.cancelled, 1);
        assert_eq!(s.outcome.unsupported, 1);
        assert_eq!(s.outcome.already_under_target, 1);
        assert_eq!(s.total_saved_bytes, 9 + 8);
    }

    #[test]
    fn fold_session_end_cancelled_flag() {
        let mut end_record = end("s1", 120);
        end_record["cancelled"] = serde_json::json!(true);
        let lines = vec![line(start("s1", 100)), line(end_record)];
        let out = fold_records(&lines);
        assert!(out.sessions[0].cancelled);
    }

    #[test]
    fn fold_orders_sessions_newest_first_and_files_by_time() {
        let lines = vec![
            line(start("old", 10)),
            line(file("old", "f1", 11, "done", (1, Some(1)))),
            line(start("new", 99)),
            line(file("new", "b", 105, "done", (1, Some(1)))),
            line(file("new", "a", 101, "done", (1, Some(1)))),
            line(end("new", 106)),
        ];
        let out = fold_records(&lines);
        let ids: Vec<&str> = out.sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, vec!["new", "old"]);
        let files = &out.sessions[0].files;
        assert_eq!(files.len(), 2);
        assert!(files[0].completed_at <= files[1].completed_at);
    }

    #[test]
    fn fold_repeated_session_start_last_wins() {
        let mut second = start("s1", 20);
        second["outputFolder"] = serde_json::json!("/out2");
        second["fileCount"] = serde_json::json!(7);
        let lines = vec![
            line(start("s1", 10)),
            line(file("s1", "f1", 11, "done", (5, Some(1)))),
            line(end("s1", 12)),
            line(second),
        ];
        let out = fold_records(&lines);
        assert_eq!(out.sessions.len(), 1);
        let s = &out.sessions[0];
        assert_eq!(s.started_at, 20);
        assert_eq!(s.output_folder, "/out2");
        assert_eq!(s.file_count, 7);
        assert_eq!(s.files.len(), 1, "prior files retained");
        assert_eq!(s.files[0].name, "a.jpg");
        assert_eq!(
            s.ended_at,
            Some(12),
            "end recorded before restart preserved"
        );
        assert_eq!(s.total_saved_bytes, 100);
    }

    #[test]
    fn fold_skips_malformed_counts_them_and_ignores_unknown_types() {
        let lines = vec![
            "not json".to_string(),
            line(serde_json::json!({ "type": "future_record", "sessionId": "x" })),
            line(serde_json::json!({ "type": "file", "sessionId": "s1" })),
            line(serde_json::json!({ "type": "session_start" })),
            line(serde_json::json!({ "type": "session_end", "sessionId": "nobody" })),
            line(start("s1", 10)),
        ];
        let out = fold_records(&lines);
        assert_eq!(
            out.skipped_lines, 4,
            "malformed and typed-invalid counted; unknown type ignored silently"
        );
        assert_eq!(out.sessions.len(), 1);
        assert!(out.sessions[0].files.is_empty());
    }

    #[test]
    fn fold_counts_non_object_and_non_string_type_lines() {
        let lines = vec![
            "[]".to_string(),
            "123".to_string(),
            "\"str\"".to_string(),
            "null".to_string(),
            "{}".to_string(),
            line(serde_json::json!({ "type": 123 })),
            line(serde_json::json!({ "type": null })),
            line(serde_json::json!({ "type": "future_record" })),
            line(start("s1", 10)),
        ];
        let out = fold_records(&lines);
        assert_eq!(
            out.skipped_lines, 7,
            "non-record lines counted as malformed"
        );
        assert_eq!(out.sessions.len(), 1);
    }

    #[test]
    fn fold_file_after_session_end_is_skipped_and_counted() {
        let lines = vec![
            line(start("s1", 10)),
            line(end("s1", 12)),
            line(file("s1", "f1", 13, "done", (5, Some(1)))),
        ];
        let out = fold_records(&lines);
        assert_eq!(out.skipped_lines, 1);
        let s = &out.sessions[0];
        assert!(s.files.is_empty(), "post-end file record not added");
        assert_eq!(s.outcome.done, 1, "end-derived outcome stays frozen");
        assert_eq!(s.total_saved_bytes, 100, "end-derived total stays frozen");
    }

    #[test]
    fn fold_orphan_file_lines_are_skipped() {
        let lines = vec![line(file("nobody", "f1", 11, "done", (5, Some(1))))];
        let out = fold_records(&lines);
        assert_eq!(out.sessions.len(), 0);
        assert_eq!(out.skipped_lines, 1);
    }

    #[test]
    fn fold_orphan_session_end_is_skipped() {
        let lines = vec![line(end("nobody", 11))];
        let out = fold_records(&lines);
        assert_eq!(out.sessions.len(), 0);
        assert_eq!(out.skipped_lines, 1);
    }

    #[test]
    fn fold_duplicate_file_id_last_wins() {
        let lines = vec![
            line(start("s1", 10)),
            line(file("s1", "f1", 11, "failed", (5, None))),
            line(file("s1", "f1", 12, "done", (5, Some(1)))),
        ];
        let out = fold_records(&lines);
        assert_eq!(out.sessions[0].files.len(), 1);
        assert_eq!(out.sessions[0].files[0].status, "done");
    }

    #[test]
    fn fold_file_ids_are_scoped_per_session() {
        let lines = vec![
            line(start("s1", 10)),
            line(file("s1", "f1", 11, "done", (5, Some(1)))),
            line(start("s2", 20)),
            line(file("s2", "f1", 21, "failed", (5, None))),
            line(file("s2", "f1", 22, "done", (5, Some(2)))),
        ];
        let out = fold_records(&lines);
        assert_eq!(out.sessions.len(), 2);
        let s2 = out
            .sessions
            .iter()
            .find(|s| s.session_id == "s2")
            .unwrap();
        let s1 = out
            .sessions
            .iter()
            .find(|s| s.session_id == "s1")
            .unwrap();
        assert_eq!(s1.files.len(), 1);
        assert_eq!(s2.files.len(), 1, "last record for f1 within s2 replaces");
        assert_eq!(s1.files[0].status, "done");
        assert_eq!(s2.files[0].status, "done", "second s2 record wins");
        assert_eq!(s2.files[0].completed_at, 22, "second s2 record's timestamp");
    }

    #[test]
    fn fold_duplicate_session_end_is_skipped_and_counted() {
        let mut second_end = end("s1", 999);
        second_end["totalSavedBytes"] = serde_json::json!(7777);
        second_end["cancelled"] = serde_json::json!(true);
        second_end["counts"] = serde_json::json!({
            "done": 0, "bestEffort": 0, "failed": 9,
            "cancelled": 0, "unsupported": 0, "alreadyUnderTarget": 0
        });
        let lines = vec![
            line(start("s1", 10)),
            line(end("s1", 100)),
            line(second_end),
        ];
        let out = fold_records(&lines);
        assert_eq!(out.skipped_lines, 1);
        let s = &out.sessions[0];
        assert_eq!(s.ended_at, Some(100), "first end wins");
        assert_eq!(s.total_saved_bytes, 100, "first end values retained");
        assert!(!s.cancelled, "first end values retained");
        assert_eq!(s.outcome.done, 1, "first end counts retained");
        assert_eq!(s.outcome.failed, 0, "second end counts ignored");
    }

    #[test]
    fn fold_saturates_when_compressed_exceeds_original() {
        let lines = vec![
            line(start("s1", 10)),
            line(file("s1", "f1", 11, "done", (5, Some(10)))),
        ];
        let out = fold_records(&lines);
        assert_eq!(out.sessions[0].total_saved_bytes, 0);
    }

    #[test]
    fn fold_interrupted_saturates_total_saved() {
        let lines = vec![
            line(start("s1", 10)),
            line(file("s1", "f1", 11, "done", (0, Some(u64::MAX)))),
            line(file("s1", "f2", 12, "done", (u64::MAX, Some(0)))),
            line(file("s1", "f3", 13, "done", (u64::MAX, Some(0)))),
        ];
        let out = fold_records(&lines);
        assert_eq!(
            out.sessions[0].total_saved_bytes,
            u64::MAX,
            "saturating_add clamps the sum instead of panicking"
        );
    }

    #[test]
    fn fold_interrupted_flag_matches_ended_at_biconditional() {
        let complete = fold_records(&[line(start("s1", 10)), line(end("s1", 12))]);
        let s = &complete.sessions[0];
        assert!(!s.interrupted);
        assert_eq!(s.ended_at, Some(12));

        let interrupted = fold_records(&[line(start("s2", 10))]);
        let s = &interrupted.sessions[0];
        assert!(s.interrupted);
        assert_eq!(s.ended_at, None);
    }

    #[test]
    fn fold_interrupted_unknown_status_not_counted() {
        let lines = vec![
            line(start("s1", 10)),
            line(file("s1", "f1", 11, "mystery", (5, Some(1)))),
        ];
        let out = fold_records(&lines);
        let s = &out.sessions[0];
        assert!(s.interrupted);
        assert_eq!(s.outcome.done, 0);
        assert_eq!(s.outcome.best_effort, 0);
        assert_eq!(s.outcome.failed, 0);
        assert_eq!(s.outcome.cancelled, 0);
        assert_eq!(s.outcome.unsupported, 0);
        assert_eq!(s.outcome.already_under_target, 0);
    }

    #[test]
    fn fold_output_serde_keys_are_pinned() {
        let lines = vec![
            line(start("s1", 100)),
            line(file("s1", "f1", 110, "done", (500, Some(100)))),
            line(end("s1", 120)),
        ];
        let out = fold_records(&lines);
        let value = serde_json::to_value(&out.sessions[0]).unwrap();
        assert_eq!(
            sorted_keys(&value),
            vec![
                "cancelled",
                "endedAt",
                "fileCount",
                "files",
                "interrupted",
                "matchedFiles",
                "outcome",
                "outputFolder",
                "sessionId",
                "startedAt",
                "totalSavedBytes",
            ]
        );
        assert_eq!(value["endedAt"], 120);
        assert!(value["matchedFiles"].is_null());
        let file = &value["files"][0];
        assert_eq!(
            sorted_keys(file),
            vec![
                "category",
                "changes",
                "completedAt",
                "compressedSize",
                "consentSource",
                "durationMs",
                "error",
                "name",
                "originalSize",
                "outputPath",
                "path",
                "qualityUsed",
                "status",
                "strategyUsed",
                "targetBytes",
            ]
        );
        assert_eq!(file["outputPath"], "/out/a.jpg");
        assert!(file["outputPath"].is_string());
        assert_eq!(file["completedAt"], 110);
        assert_eq!(file["qualityUsed"], 70);
        assert_eq!(file["strategyUsed"], "quality=70");
        assert_eq!(file["durationMs"], 12);
    }

    #[test]
    fn fold_tolerates_absent_optional_keys() {
        let minimal = serde_json::json!({
            "type": "file", "sessionId": "s1", "fileId": "f1", "completedAt": 11,
            "name": "a.jpg", "path": "/in/a.jpg", "category": "image",
            "targetBytes": 1024, "originalSize": 5, "status": "failed", "durationMs": 0
        });
        let lines = vec![line(start("s1", 10)), line(minimal)];
        let out = fold_records(&lines);
        assert_eq!(out.skipped_lines, 0);
        assert_eq!(out.sessions[0].files.len(), 1);
        let f = &out.sessions[0].files[0];
        assert_eq!(f.output_path, None);
        assert_eq!(f.compressed_size, None);
        assert_eq!(f.quality_used, None);
        assert_eq!(f.strategy_used, None);
        assert_eq!(f.error, None);
    }

    #[test]
    fn fold_zero_file_interrupted_session_still_appears() {
        let lines = vec![line(start("s1", 10))];
        let out = fold_records(&lines);
        assert_eq!(out.sessions.len(), 1);
        assert!(out.sessions[0].files.is_empty());
        assert!(out.sessions[0].interrupted);
        assert!(!out.sessions[0].cancelled);
        assert_eq!(out.sessions[0].ended_at, None);
        assert_eq!(out.sessions[0].outcome, Counts::default());
    }

    #[test]
    fn load_roundtrips_through_file() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store
            .append_session_start(&SessionStartRecord {
                session_id: "s1".into(),
                started_at: 10,
                file_count: 1,
                output_folder: "/out".into(),
            })
            .unwrap();
        store
            .append_file(&FileResultRecord {
                session_id: "s1".into(),
                file_id: "f1".into(),
                completed_at: 11,
                name: "a.jpg".into(),
                path: "/in/a.jpg".into(),
                output_path: Some("/out/a.jpg".into()),
                category: "image".into(),
                target_bytes: 1024,
                original_size: 5,
                compressed_size: Some(1),
                status: "done".into(),
                quality_used: Some(70),
                strategy_used: Some("quality=70".into()),
                error: None,
                duration_ms: 12,
                changes: Vec::new(),
                consent_source: None,
            })
            .unwrap();
        let out = store.load().unwrap();
        assert_eq!(out.skipped_lines, 0);
        assert_eq!(out.sessions[0].session_id, "s1");
        assert_eq!(out.sessions[0].files[0].compressed_size, Some(1));
    }

    #[test]
    fn load_counts_lossy_non_json_line_as_skipped() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(store.file_path())
            .unwrap();
        file.write_all(&[0xff, 0xfe, b'\n']).unwrap();
        let start_line = line(start("s1", 10));
        file.write_all(start_line.as_bytes()).unwrap();
        file.write_all(b"\n").unwrap();

        let out = store.load().unwrap();
        assert_eq!(out.skipped_lines, 1, "lossy line is not valid JSON");
        assert_eq!(out.sessions.len(), 1);
        assert_eq!(out.sessions[0].session_id, "s1");
    }

    #[test]
    fn load_surfaces_read_error() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        std::fs::create_dir(store.file_path()).unwrap();
        let err = store.load().unwrap_err();
        assert!(err.starts_with("history read failed: read "), "{err}");
    }

    fn start_in(sid: &str, at: i64, folder: &str) -> String {
        line(serde_json::json!({
            "type": "session_start", "sessionId": sid, "startedAt": at,
            "fileCount": 1, "outputFolder": folder
        }))
    }

    fn qfile(
        sid: &str,
        fid: &str,
        at: i64,
        name: &str,
        path: &str,
        output_path: Option<&str>,
        category: &str,
        status: &str,
        sizes: (u64, Option<u64>),
        error: Option<&str>,
    ) -> String {
        line(serde_json::json!({
            "type": "file", "sessionId": sid, "fileId": fid, "completedAt": at,
            "name": name, "path": path, "outputPath": output_path,
            "category": category, "targetBytes": 1024,
            "originalSize": sizes.0, "compressedSize": sizes.1,
            "status": status, "qualityUsed": 70, "strategyUsed": "quality=70",
            "error": error, "durationMs": 12
        }))
    }

    fn two_sessions() -> FoldResult {
        let lines = vec![
            start_in("s2", 200, "/outB"),
            line(file("s2", "f2", 201, "failed", (100, None))),
            line(start("s1", 100)),
            line(file("s1", "f1", 101, "done", (500, Some(100)))),
            line(end("s1", 102)),
        ];
        fold_records(&lines)
    }

    fn five_sessions() -> FoldResult {
        let lines = vec![
            start_in("s5", 500, "/out"),
            start_in("s4", 400, "/out"),
            start_in("s3", 300, "/out"),
            start_in("s2", 200, "/out"),
            start_in("s1", 100, "/out"),
        ];
        fold_records(&lines)
    }

    fn five_sessions_mixed_statuses() -> FoldResult {
        let lines = vec![
            start_in("s5", 500, "/out"),
            qfile("s5", "f1", 501, "a.jpg", "/in/a.jpg", Some("/out/a.jpg"), "image", "done", (10, Some(5)), None),
            start_in("s4", 400, "/out"),
            qfile("s4", "f1", 401, "b.jpg", "/in/b.jpg", None, "image", "failed", (10, None), Some("boom")),
            start_in("s3", 300, "/out"),
            qfile("s3", "f1", 301, "c.jpg", "/in/c.jpg", Some("/out/c.jpg"), "image", "done", (10, Some(5)), None),
            start_in("s2", 200, "/out"),
            qfile("s2", "f1", 201, "d.jpg", "/in/d.jpg", None, "image", "failed", (10, None), Some("boom")),
            start_in("s1", 100, "/out"),
            qfile("s1", "f1", 101, "e.jpg", "/in/e.jpg", None, "image", "failed", (10, None), Some("boom")),
        ];
        fold_records(&lines)
    }

    fn search_fixture() -> FoldResult {
        let lines = vec![
            start_in("alpha", 100, "/out/alpha"),
            qfile(
                "alpha",
                "f1",
                101,
                "sunset.jpg",
                "/photos/sunset.jpg",
                Some("/out/alpha/sunset.jpg"),
                "image",
                "done",
                (500, Some(100)),
                None,
            ),
            start_in("beta", 200, "/out/beta"),
            qfile(
                "beta",
                "f2",
                201,
                "report.pdf",
                "/docs/report.pdf",
                None,
                "pdf",
                "failed",
                (100, None),
                Some("cannot read locked file"),
            ),
            start_in("gamma", 300, "/out/gamma"),
            qfile(
                "gamma",
                "f3",
                301,
                "sunset.jpg",
                "/in/zzz.bin",
                None,
                "image",
                "done",
                (500, Some(100)),
                None,
            ),
        ];
        fold_records(&lines)
    }

    fn folder_and_file_hit_fixture() -> FoldResult {
        let lines = vec![
            start_in("s1", 10, "/projects/alpha"),
            qfile(
                "s1",
                "f1",
                11,
                "alpha.txt",
                "/in/alpha.txt",
                Some("/projects/alpha/alpha.txt"),
                "document",
                "done",
                (100, Some(50)),
                None,
            ),
            qfile(
                "s1",
                "f2",
                12,
                "beta.txt",
                "/in/beta.txt",
                None,
                "document",
                "failed",
                (100, None),
                Some("nope"),
            ),
        ];
        fold_records(&lines)
    }

    fn old_start_new_file_fixture() -> FoldResult {
        let lines = vec![
            start_in("old", 100, "/out"),
            qfile(
                "old",
                "f1",
                500,
                "a.jpg",
                "/in/a.jpg",
                Some("/out/a.jpg"),
                "image",
                "done",
                (10, Some(5)),
                None,
            ),
            start_in("new", 300, "/out"),
            qfile(
                "new",
                "f1",
                301,
                "a.jpg",
                "/in/a.jpg",
                Some("/out/a.jpg"),
                "image",
                "done",
                (10, Some(5)),
                None,
            ),
            qfile(
                "new",
                "f2",
                50,
                "early.jpg",
                "/in/early.jpg",
                Some("/out/early.jpg"),
                "image",
                "done",
                (10, Some(5)),
                None,
            ),
        ];
        fold_records(&lines)
    }

    fn two_sessions_two_files_each() -> FoldResult {
        let lines = vec![
            start_in("s2", 200, "/out"),
            qfile("s2", "a", 201, "a.jpg", "/in/a.jpg", Some("/out/a.jpg"), "image", "done", (10, Some(5)), None),
            qfile("s2", "b", 202, "b.jpg", "/in/b.jpg", Some("/out/b.jpg"), "image", "done", (10, Some(5)), None),
            start_in("s1", 100, "/out"),
            qfile("s1", "a", 101, "a.jpg", "/in/a.jpg", Some("/out/a.jpg"), "image", "done", (10, Some(5)), None),
            qfile("s1", "b", 102, "b.jpg", "/in/b.jpg", Some("/out/b.jpg"), "image", "done", (10, Some(5)), None),
        ];
        fold_records(&lines)
    }

    fn mixed_folders() -> FoldResult {
        let lines = vec![
            start_in("a", 400, "/b"),
            start_in("b", 300, "/a"),
            start_in("c", 200, "/b"),
            start_in("d", 100, ""),
        ];
        fold_records(&lines)
    }

    fn no_filters() -> HistoryFilters {
        HistoryFilters::default()
    }

    fn sorted_ids(fold: FoldResult, filters: &HistoryFilters) -> Vec<String> {
        query_history(fold, filters, 0, 50)
            .sessions
            .into_iter()
            .map(|session| session.session_id)
            .collect()
    }

    fn sort_filter(sort: &str) -> HistoryFilters {
        HistoryFilters {
            sort: Some(sort.into()),
            ..no_filters()
        }
    }

    fn scrambled_sessions() -> FoldResult {
        let mut fold = five_sessions_mixed_statuses();
        let mut sessions = Vec::new();
        for id in ["s3", "s1", "s5", "s2", "s4"] {
            let index = fold
                .sessions
                .iter()
                .position(|session| session.session_id == id)
                .unwrap();
            sessions.push(fold.sessions.remove(index));
        }
        FoldResult {
            sessions,
            skipped_lines: fold.skipped_lines,
        }
    }

    fn bare_session(session_id: &str, started_at: i64) -> HistorySession {
        HistorySession {
            session_id: session_id.into(),
            started_at,
            ended_at: None,
            output_folder: "/out".into(),
            file_count: 0,
            files: Vec::new(),
            outcome: Counts::default(),
            total_saved_bytes: 0,
            cancelled: false,
            interrupted: true,
            matched_files: None,
        }
    }

    #[test]
    fn sort_newest_is_default() {
        assert_eq!(
            sorted_ids(scrambled_sessions(), &sort_filter("newest")),
            vec!["s5", "s4", "s3", "s2", "s1"]
        );
        assert_eq!(
            sorted_ids(scrambled_sessions(), &no_filters()),
            vec!["s5", "s4", "s3", "s2", "s1"],
            "absent sort keeps the newest-first default"
        );
    }

    #[test]
    fn sort_oldest_reverses_started_at() {
        assert_eq!(
            sorted_ids(scrambled_sessions(), &sort_filter("oldest")),
            vec!["s1", "s2", "s3", "s4", "s5"]
        );
    }

    #[test]
    fn sort_unknown_falls_back_to_newest() {
        for sort in ["recent", "", "OLDEST"] {
            assert_eq!(
                sorted_ids(scrambled_sessions(), &sort_filter(sort)),
                vec!["s5", "s4", "s3", "s2", "s1"],
                "unknown sort {sort:?} falls back to newest"
            );
        }
    }

    #[test]
    fn effective_sort_normalizes_values() {
        assert_eq!(effective_sort(None), HistorySort::Newest);
        assert_eq!(effective_sort(Some("")), HistorySort::Newest);
        assert_eq!(effective_sort(Some("   ")), HistorySort::Newest);
        assert_eq!(effective_sort(Some("newest")), HistorySort::Newest);
        assert_eq!(effective_sort(Some("  newest  ")), HistorySort::Newest);
        assert_eq!(effective_sort(Some("oldest")), HistorySort::Oldest);
        assert_eq!(effective_sort(Some(" oldest ")), HistorySort::Oldest);
        assert_eq!(effective_sort(Some("OLDEST")), HistorySort::Newest);
        assert_eq!(effective_sort(Some("recent")), HistorySort::Newest);
    }

    #[test]
    fn sort_ties_keep_every_session_across_pages() {
        let fold = || FoldResult {
            sessions: (0..40)
                .map(|i| bare_session(&format!("t{i}"), (i as i64 % 4) * 10))
                .collect(),
            skipped_lines: 0,
        };
        let expected = |groups: [i64; 4]| -> Vec<String> {
            let mut ids = Vec::new();
            for group in groups {
                ids.extend(
                    (0..40)
                        .filter(|i| ((*i % 4) as i64) * 10 == group)
                        .map(|i| format!("t{i}")),
                );
            }
            ids
        };
        let newest = expected([30, 20, 10, 0]);
        let oldest = expected([0, 10, 20, 30]);
        let mut newest_reversed = newest.clone();
        newest_reversed.reverse();
        assert_ne!(
            oldest, newest_reversed,
            "oldest is not newest reversed: ties keep fold order in both directions"
        );
        for (sort, want) in [("newest", &newest), ("oldest", &oldest)] {
            let mut ids = Vec::new();
            for page in 0..8 {
                let resp = query_history(fold(), &sort_filter(sort), page, 5);
                assert_eq!(resp.sessions.len(), 5, "sort {sort:?} page {page}");
                assert_eq!(resp.total_sessions, 40);
                ids.extend(
                    resp.sessions
                        .into_iter()
                        .map(|session| session.session_id),
                );
            }
            assert_eq!(ids.len(), 40, "sort {sort:?} walks every session once");
            assert_eq!(
                &ids, want,
                "sort {sort:?} is stable: ties stay in fold order across pages"
            );
        }
    }

    #[test]
    fn sort_oldest_applies_after_status_filter() {
        let filters = HistoryFilters {
            statuses: Some(vec!["failed".into()]),
            sort: Some("oldest".into()),
            ..no_filters()
        };
        let mut ids = Vec::new();
        for page in 0..3 {
            let resp = query_history(scrambled_sessions(), &filters, page, 1);
            assert_eq!(resp.sessions.len(), 1, "page {page}");
            assert_eq!(resp.total_sessions, 3);
            assert!(
                resp.sessions[0]
                    .files
                    .iter()
                    .all(|file| file.status == "failed"),
                "only sessions surviving the status filter appear"
            );
            ids.extend(
                resp.sessions
                    .into_iter()
                    .map(|session| session.session_id),
            );
        }
        assert_eq!(ids, vec!["s1", "s2", "s4"]);
    }

    #[test]
    fn sort_applies_before_pagination() {
        let newest = query_history(scrambled_sessions(), &sort_filter("newest"), 0, 1);
        let oldest = query_history(scrambled_sessions(), &sort_filter("oldest"), 0, 1);
        assert_eq!(newest.sessions.len(), 1);
        assert_eq!(oldest.sessions.len(), 1);
        assert_eq!(newest.sessions[0].session_id, "s5");
        assert_eq!(oldest.sessions[0].session_id, "s1");
        assert_eq!(newest.total_sessions, 5);
        assert_eq!(oldest.total_sessions, 5);
        assert_eq!(newest.total_files, 5);
        assert_eq!(oldest.total_files, newest.total_files);
        assert_eq!(oldest.summary, newest.summary);
        let oldest_second = query_history(scrambled_sessions(), &sort_filter("oldest"), 1, 1);
        assert_eq!(
            oldest_second.sessions[0].session_id, "s2",
            "pagination slices after the sort"
        );
    }

    #[test]
    fn query_returns_first_page_and_totals() {
        let resp = query_history(two_sessions(), &no_filters(), 0, 1);
        assert_eq!(resp.page, 0);
        assert_eq!(resp.page_size, 1);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].session_id, "s2");
        assert_eq!(resp.total_sessions, 2);
        assert_eq!(resp.total_files, 2);
        assert_eq!(resp.summary.sessions, 2);
        assert_eq!(resp.summary.failed_files, 1);
        assert_eq!(resp.summary.saved_bytes, 400);
    }

    #[test]
    fn query_page_middle_and_last_partial_page() {
        let expected: [(u32, usize, &[&str]); 3] = [
            (0, 2, &["s5", "s4"]),
            (1, 2, &["s3", "s2"]),
            (2, 1, &["s1"]),
        ];
        for (page, want_len, want_ids) in expected {
            let resp = query_history(five_sessions(), &no_filters(), page, 2);
            assert_eq!(resp.page, page);
            assert_eq!(resp.page_size, 2);
            assert_eq!(resp.sessions.len(), want_len, "page {page}");
            let ids: Vec<&str> = resp.sessions.iter().map(|s| s.session_id.as_str()).collect();
            assert_eq!(ids, want_ids, "page {page}");
        }
    }

    #[test]
    fn query_page_beyond_end_clamps_to_last_page() {
        let resp = query_history(two_sessions(), &no_filters(), 9, 1);
        assert_eq!(resp.page, 1);
        assert_eq!(resp.sessions[0].session_id, "s1");
    }

    #[test]
    fn query_empty_result_is_page_zero() {
        let filters = HistoryFilters { search: Some("definitely-absent".into()), ..no_filters() };
        let resp = query_history(two_sessions(), &filters, 9, 50);
        assert_eq!(resp.page, 0);
        assert!(resp.sessions.is_empty());
        assert_eq!(resp.total_sessions, 0);
        assert_eq!(resp.total_files, 0);
        assert_eq!(
            resp.summary,
            HistorySummary { sessions: 0, files: 0, saved_bytes: 0, failed_files: 0 }
        );
        assert_eq!(
            resp.output_folders,
            vec!["/out".to_string(), "/outB".to_string()]
        );
    }

    #[test]
    fn query_page_size_zero_is_safe() {
        if cfg!(debug_assertions) {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                query_history(two_sessions(), &no_filters(), 0, 0);
            }));
            assert!(
                result.is_err(),
                "debug builds catch page_size 0 through the query_history debug_assert contract"
            );
        } else {
            let resp = query_history(two_sessions(), &no_filters(), 0, 0);
            assert_eq!(resp.page_size, 1);
            assert_eq!(resp.sessions.len(), 1);
            assert_eq!(resp.sessions[0].session_id, "s2");
            let clamped = query_history(two_sessions(), &no_filters(), 9, 0);
            assert_eq!(clamped.page, 1);
            assert_eq!(clamped.sessions[0].session_id, "s1");
        }
    }

    #[test]
    fn query_status_filter_reduces_files_and_counts_them() {
        let filters = HistoryFilters {
            statuses: Some(vec!["failed".into()]),
            ..no_filters()
        };
        let resp = query_history(two_sessions(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].files.len(), 1);
        assert_eq!(resp.sessions[0].matched_files, Some(1));
        assert_eq!(resp.summary.files, 1);
        assert_eq!(resp.total_files, 1);
    }

    #[test]
    fn query_category_filter() {
        let filters = HistoryFilters {
            categories: Some(vec!["video".into()]),
            ..no_filters()
        };
        let resp = query_history(two_sessions(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 0);
        let filters = HistoryFilters {
            categories: Some(vec!["image".into()]),
            ..no_filters()
        };
        let resp = query_history(two_sessions(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 2);
        assert_eq!(resp.sessions[0].matched_files, Some(1));
        let substring = HistoryFilters {
            categories: Some(vec!["imag".into()]),
            ..no_filters()
        };
        let resp = query_history(two_sessions(), &substring, 0, 50);
        assert_eq!(
            resp.sessions.len(),
            0,
            "category filter is exact, not a substring match"
        );
    }

    #[test]
    fn query_search_hits_name_source_path_output_path_error_and_folder() {
        for (query, want_ids) in [
            ("SUNSET", vec!["gamma", "alpha"]),
            ("photos/sunset", vec!["alpha"]),
            ("alpha/sunset", vec!["alpha"]),
            ("LOCKED", vec!["beta"]),
            ("/out/beta", vec!["beta"]),
            ("no-such-text", Vec::new()),
        ] {
            let filters = HistoryFilters { search: Some(query.into()), ..no_filters() };
            let resp = query_history(search_fixture(), &filters, 0, 50);
            let ids: Vec<&str> = resp.sessions.iter().map(|s| s.session_id.as_str()).collect();
            assert_eq!(ids, want_ids, "query {query:?}");
        }
        let name_only = HistoryFilters { search: Some("SUNSET".into()), ..no_filters() };
        let resp = query_history(search_fixture(), &name_only, 0, 50);
        let gamma = resp
            .sessions
            .iter()
            .find(|s| s.session_id == "gamma")
            .expect("name-only hit is found even when path/outputPath/error do not match");
        assert_eq!(gamma.files.len(), 1);
        assert_eq!(gamma.matched_files, Some(1));
        let folder_only = HistoryFilters { search: Some("/out/beta".into()), ..no_filters() };
        let resp = query_history(search_fixture(), &folder_only, 0, 50);
        assert_eq!(resp.sessions[0].matched_files, None, "folder-only hit keeps all files");
        assert_eq!(resp.sessions[0].files.len(), 1);
        let file_hit = HistoryFilters { search: Some("locked".into()), ..no_filters() };
        let resp = query_history(search_fixture(), &file_hit, 0, 50);
        assert_eq!(resp.sessions[0].matched_files, Some(1));
    }

    #[test]
    fn query_search_file_and_folder_hit_file_set_wins() {
        let filters = HistoryFilters { search: Some("alpha".into()), ..no_filters() };
        let resp = query_history(folder_and_file_hit_fixture(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].matched_files, Some(1));
        let names: Vec<&str> = resp.sessions[0].files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["alpha.txt"]);
        assert_eq!(resp.summary.files, 1);
    }

    #[test]
    fn query_folder_search_with_status_filter_combines_semantics() {
        let filters = HistoryFilters {
            search: Some("outb".into()),
            statuses: Some(vec!["failed".into()]),
            ..no_filters()
        };
        let resp = query_history(two_sessions(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].session_id, "s2");
        assert_eq!(resp.sessions[0].matched_files, Some(1));

        let filters = HistoryFilters {
            search: Some("outb".into()),
            statuses: Some(vec!["done".into()]),
            ..no_filters()
        };
        assert_eq!(query_history(two_sessions(), &filters, 0, 50).sessions.len(), 0);
    }

    #[test]
    fn query_empty_and_whitespace_search_are_inactive() {
        for value in ["", "  ", "\t"] {
            let filters = HistoryFilters { search: Some(value.into()), ..no_filters() };
            let resp = query_history(two_sessions(), &filters, 0, 50);
            assert_eq!(resp.sessions.len(), 2, "search {value:?} is inactive");
            assert_eq!(resp.sessions[0].matched_files, None);
        }
    }

    #[test]
    fn query_empty_status_and_category_vecs_are_inactive() {
        let filters = HistoryFilters {
            statuses: Some(Vec::new()),
            categories: Some(Vec::new()),
            ..no_filters()
        };
        let resp = query_history(two_sessions(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 2, "toggling the last chip off shows everything");
        assert_eq!(resp.sessions[0].matched_files, None);
    }

    #[test]
    fn query_since_ms_inclusive_boundary() {
        let exact = HistoryFilters { since_ms: Some(100), ..no_filters() };
        let resp = query_history(two_sessions(), &exact, 0, 50);
        assert_eq!(resp.sessions.len(), 2);
        assert!(
            resp.sessions.iter().all(|session| session.matched_files.is_none()),
            "since_ms is session-level only; it must never reduce the file set"
        );
        let later = HistoryFilters { since_ms: Some(101), ..no_filters() };
        let resp = query_history(two_sessions(), &later, 0, 50);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].session_id, "s2");
        assert_eq!(resp.sessions[0].matched_files, None);
        assert_eq!(resp.sessions[0].files.len(), 1, "files untouched by the date filter");
    }

    #[test]
    fn query_since_ms_uses_session_start_not_file_times() {
        let filters = HistoryFilters { since_ms: Some(200), ..no_filters() };
        let resp = query_history(old_start_new_file_fixture(), &filters, 0, 50);
        assert_eq!(
            resp.sessions.len(),
            1,
            "old session with a fresh file is still excluded"
        );
        assert_eq!(resp.sessions[0].session_id, "new");
        assert_eq!(resp.sessions[0].matched_files, None);

        let inclusive = HistoryFilters { since_ms: Some(100), ..no_filters() };
        let resp = query_history(old_start_new_file_fixture(), &inclusive, 0, 50);
        assert_eq!(resp.sessions.len(), 2, "old session included at the boundary");
        let old = resp
            .sessions
            .iter()
            .find(|s| s.session_id == "old")
            .expect("old session present");
        assert_eq!(
            old.files.len(),
            1,
            "since_ms never filters file records; files stay untouched"
        );
        assert_eq!(old.matched_files, None, "since_ms is session-level only");

        let session_incl_file_before = HistoryFilters { since_ms: Some(200), ..no_filters() };
        let resp = query_history(old_start_new_file_fixture(), &session_incl_file_before, 0, 50);
        let new = resp
            .sessions
            .iter()
            .find(|s| s.session_id == "new")
            .expect("new session present");
        assert_eq!(
            new.files.len(),
            2,
            "a file completed before the cutoff does not shrink an included session's file set"
        );
        assert_eq!(
            new.matched_files, None,
            "a mutant applying since_ms to files without setting matched_files must fail"
        );
    }

    #[test]
    fn query_combined_filters() {
        let filters = HistoryFilters {
            since_ms: Some(100),
            statuses: Some(vec!["done".into()]),
            output_folder: Some("/out".into()),
            ..no_filters()
        };
        let resp = query_history(two_sessions(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].session_id, "s1");
        assert_eq!(resp.sessions[0].matched_files, Some(1));
        assert_eq!(resp.summary.files, 1);

        let no_match = HistoryFilters {
            statuses: Some(vec!["failed".into()]),
            output_folder: Some("/out".into()),
            ..no_filters()
        };
        assert_eq!(query_history(two_sessions(), &no_match, 0, 50).sessions.len(), 0);
    }

    #[test]
    fn query_summary_is_page_independent() {
        let first = query_history(two_sessions(), &no_filters(), 0, 1);
        let second = query_history(two_sessions(), &no_filters(), 1, 1);
        assert_eq!(second.page, 1);
        assert_eq!(second.sessions.len(), 1);
        assert_eq!(second.sessions[0].session_id, "s1");
        assert_eq!(first.total_sessions, second.total_sessions);
        assert_eq!(first.total_files, second.total_files);
        assert_eq!(first.summary, second.summary);
        assert_eq!(second.total_files, 2);
        assert_eq!(second.summary.saved_bytes, 400);
    }

    #[test]
    fn query_never_splits_session_across_pages() {
        let resp = query_history(two_sessions_two_files_each(), &no_filters(), 0, 1);
        assert_eq!(resp.page_size, 1);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].session_id, "s2");
        assert_eq!(resp.sessions[0].files.len(), 2, "both files travel with the session row");
        let resp = query_history(two_sessions_two_files_each(), &no_filters(), 1, 1);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].session_id, "s1");
        assert_eq!(resp.sessions[0].files.len(), 2);
    }

    #[test]
    fn query_output_folders_list_is_unfiltered_deduped_sorted() {
        let filters = HistoryFilters { output_folder: Some("/b".into()), ..no_filters() };
        let resp = query_history(mixed_folders(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 2);
        let ids: Vec<&str> = resp.sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c"]);
        assert_eq!(
            resp.output_folders,
            vec!["/a".to_string(), "/b".to_string()],
            "unfiltered, deduped, sorted, non-empty"
        );
    }

    #[test]
    fn query_output_folder_filter_excludes_empty_folder_sessions() {
        let unfiltered = query_history(mixed_folders(), &no_filters(), 0, 50);
        assert_eq!(
            unfiltered.sessions.len(),
            4,
            "empty-folder session visible without a folder filter"
        );
        let filters = HistoryFilters { output_folder: Some("/b".into()), ..no_filters() };
        let resp = query_history(mixed_folders(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 2);
        assert!(resp.sessions.iter().all(|s| s.output_folder == "/b"));
        assert!(
            resp.sessions.iter().all(|s| s.matched_files.is_none()),
            "folder filter is session-level; matchedFiles stays None"
        );
        assert!(!resp.sessions.iter().any(|s| s.session_id == "d"));
    }

    fn folder_filter_sessions_with_files() -> FoldResult {
        let lines = vec![
            start_in("a", 400, "/b"),
            qfile("a", "f1", 401, "a.jpg", "/in/a.jpg", Some("/out/a.jpg"), "image", "done", (10, Some(5)), None),
            start_in("b", 300, "/a"),
            qfile("b", "f1", 301, "b.jpg", "/in/b.jpg", Some("/out/b.jpg"), "image", "failed", (10, None), Some("boom")),
            start_in("c", 200, "/b"),
            qfile("c", "f1", 201, "c.jpg", "/in/c.jpg", Some("/out/c.jpg"), "image", "done", (10, Some(5)), None),
        ];
        fold_records(&lines)
    }

    #[test]
    fn query_output_folder_filter_is_session_level() {
        let filters = HistoryFilters { output_folder: Some("/b".into()), ..no_filters() };
        let resp = query_history(folder_filter_sessions_with_files(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 2);
        assert!(
            resp.sessions.iter().all(|s| s.matched_files.is_none()),
            "folder filter is session-level; it must never set matchedFiles"
        );
        assert!(
            resp.sessions.iter().all(|s| s.files.len() == 1),
            "file sets stay untouched under a folder filter"
        );
        assert_eq!(resp.summary.files, 2);
    }

    #[test]
    fn query_page_beyond_end_clamps_with_active_filter() {
        let filters = HistoryFilters {
            statuses: Some(vec!["failed".into()]),
            ..no_filters()
        };
        let resp = query_history(five_sessions_mixed_statuses(), &filters, 9, 1);
        assert_eq!(
            resp.page, 2,
            "page clamps to the last page of the filtered set, not the unfiltered set"
        );
        assert_eq!(resp.sessions.len(), 1, "clamped page still returns a session");
        assert_eq!(resp.sessions[0].session_id, "s1");
        assert_eq!(resp.total_sessions, 3, "three of five sessions are failed");
    }

    fn three_status_session() -> FoldResult {
        let lines = vec![
            start_in("s1", 10, "/out"),
            qfile("s1", "f1", 11, "a.jpg", "/in/a.jpg", Some("/out/a.jpg"), "image", "done", (10, Some(5)), None),
            qfile("s1", "f2", 12, "b.jpg", "/in/b.jpg", Some("/out/b.jpg"), "image", "failed", (10, None), Some("boom")),
            qfile("s1", "f3", 13, "c.mp4", "/in/c.mp4", Some("/out/c.mp4"), "video", "cancelled", (10, None), None),
        ];
        fold_records(&lines)
    }

    #[test]
    fn query_multi_status_filters_are_a_union() {
        let filters = HistoryFilters {
            statuses: Some(vec!["failed".into(), "cancelled".into()]),
            ..no_filters()
        };
        let resp = query_history(three_status_session(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 1, "union keeps the session");
        assert_eq!(resp.sessions[0].files.len(), 2);
        assert!(resp.sessions[0].files.iter().all(|f| f.status != "done"));
        assert_eq!(resp.sessions[0].matched_files, Some(2));
        assert_eq!(resp.summary.files, 2);

        let categories = HistoryFilters {
            categories: Some(vec!["pdf".into(), "video".into()]),
            ..no_filters()
        };
        let resp = query_history(three_status_session(), &categories, 0, 50);
        assert_eq!(resp.sessions.len(), 1, "second category element matches");
        assert_eq!(resp.sessions[0].files.len(), 1);
        assert_eq!(resp.sessions[0].files[0].category, "video");
        assert_eq!(resp.sessions[0].matched_files, Some(1));
        assert_eq!(resp.summary.files, 1);
    }

    fn planned_three_actual_two() -> FoldResult {
        let lines = vec![
            line(serde_json::json!({
                "type": "session_start", "sessionId": "s1", "startedAt": 10,
                "fileCount": 3, "outputFolder": "/out"
            })),
            qfile("s1", "f1", 11, "a.jpg", "/in/a.jpg", Some("/out/a.jpg"), "image", "done", (10, Some(5)), None),
            qfile("s1", "f2", 12, "b.jpg", "/in/b.jpg", Some("/out/b.jpg"), "image", "failed", (10, None), Some("boom")),
        ];
        fold_records(&lines)
    }

    #[test]
    fn query_matched_files_uses_returned_len_not_planned_file_count() {
        let filters = HistoryFilters {
            statuses: Some(vec!["failed".into()]),
            ..no_filters()
        };
        let resp = query_history(planned_three_actual_two(), &filters, 0, 50);
        assert_eq!(resp.sessions[0].file_count, 3, "planned count stays on the session");
        assert_eq!(resp.sessions[0].files.len(), 1);
        assert_eq!(
            resp.sessions[0].matched_files,
            Some(1),
            "matchedFiles is the returned file count, never the planned fileCount"
        );
        assert_eq!(resp.summary.files, 1);
    }

    fn done_and_failed_session() -> FoldResult {
        let lines = vec![
            start_in("s1", 10, "/out"),
            qfile("s1", "f1", 11, "a.jpg", "/in/a.jpg", Some("/out/a.jpg"), "image", "done", (10, Some(5)), None),
            qfile("s1", "f2", 12, "b.jpg", "/in/b.jpg", None, "image", "failed", (10, None), Some("boom")),
            line(end("s1", 13)),
        ];
        fold_records(&lines)
    }

    #[test]
    fn query_summary_failed_files_respects_file_filter() {
        let done_only = HistoryFilters { statuses: Some(vec!["done".into()]), ..no_filters() };
        let resp = query_history(done_and_failed_session(), &done_only, 0, 50);
        assert_eq!(resp.sessions[0].files.len(), 1);
        assert_eq!(resp.summary.failed_files, 0, "only displayed files count");

        let failed_only = HistoryFilters { statuses: Some(vec!["failed".into()]), ..no_filters() };
        let resp = query_history(done_and_failed_session(), &failed_only, 0, 50);
        assert_eq!(resp.summary.failed_files, 1);

        let unfiltered = query_history(done_and_failed_session(), &no_filters(), 0, 50);
        assert_eq!(unfiltered.sessions[0].outcome.failed, 0, "end-record counts disagree");
        assert_eq!(
            unfiltered.summary.failed_files, 1,
            "summary comes from displayed files, not session outcome"
        );
    }

    fn search_status_fixture() -> FoldResult {
        let lines = vec![
            start_in("s1", 10, "/out"),
            qfile("s1", "f1", 11, "a.jpg", "/in/a.jpg", Some("/out/a.jpg"), "image", "failed", (10, Some(5)), Some("locked")),
            qfile("s1", "f2", 12, "b.jpg", "/in/b.jpg", None, "image", "failed", (10, None), Some("boom")),
            qfile("s1", "f3", 13, "c.jpg", "/in/c.jpg", Some("/out/c.jpg"), "image", "done", (10, Some(5)), Some("locked")),
        ];
        fold_records(&lines)
    }

    #[test]
    fn query_file_search_hit_intersects_status_filter() {
        let search_only = HistoryFilters { search: Some("locked".into()), ..no_filters() };
        let resp = query_history(search_status_fixture(), &search_only, 0, 50);
        assert_eq!(resp.sessions.len(), 1, "search alone keeps the session");
        assert_eq!(resp.sessions[0].files.len(), 2, "f1 and f3 match the searched error");
        assert_eq!(resp.sessions[0].matched_files, Some(2));

        let status_only = HistoryFilters {
            statuses: Some(vec!["failed".into()]),
            ..no_filters()
        };
        let resp = query_history(search_status_fixture(), &status_only, 0, 50);
        assert_eq!(resp.sessions[0].files.len(), 2, "f1 and f2 are failed");

        let filters = HistoryFilters {
            search: Some("locked".into()),
            statuses: Some(vec!["failed".into()]),
            ..no_filters()
        };
        let resp = query_history(search_status_fixture(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(
            resp.sessions[0].files.len(),
            1,
            "search ∩ status: status narrows within the search and search narrows within the status"
        );
        assert_eq!(resp.sessions[0].files[0].name, "a.jpg");
        assert_eq!(resp.sessions[0].matched_files, Some(1));
        assert_eq!(resp.summary.files, 1);
    }

    fn pdf_and_image_errors_fixture() -> FoldResult {
        let lines = vec![
            start_in("s1", 10, "/out"),
            qfile("s1", "f1", 11, "a.pdf", "/in/a.pdf", Some("/out/a.pdf"), "pdf", "failed", (10, None), Some("locked")),
            qfile("s1", "f2", 12, "b.pdf", "/in/b.pdf", Some("/out/b.pdf"), "pdf", "failed", (10, None), Some("boom")),
            qfile("s1", "f3", 13, "c.jpg", "/in/c.jpg", Some("/out/c.jpg"), "image", "failed", (10, None), Some("locked")),
        ];
        fold_records(&lines)
    }

    #[test]
    fn query_file_search_hit_intersects_category_filter() {
        let filters = HistoryFilters {
            search: Some("locked".into()),
            categories: Some(vec!["pdf".into()]),
            ..no_filters()
        };
        let resp = query_history(pdf_and_image_errors_fixture(), &filters, 0, 50);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(
            resp.sessions[0].files.len(),
            1,
            "search ∩ category: category narrows within the search and search narrows within the category"
        );
        assert_eq!(resp.sessions[0].files[0].name, "a.pdf");
        assert_eq!(resp.sessions[0].matched_files, Some(1));
        assert_eq!(resp.summary.files, 1);
    }

    fn two_max_saved_files() -> FoldResult {
        let lines = vec![
            start_in("s1", 10, "/out"),
            qfile("s1", "f1", 11, "a.jpg", "/in/a.jpg", Some("/out/a.jpg"), "image", "done", (u64::MAX, Some(0)), None),
            qfile("s1", "f2", 12, "b.jpg", "/in/b.jpg", Some("/out/b.jpg"), "image", "done", (u64::MAX, Some(0)), None),
        ];
        fold_records(&lines)
    }

    #[test]
    fn query_summary_saved_bytes_saturates() {
        let resp = query_history(two_max_saved_files(), &no_filters(), 0, 50);
        assert_eq!(resp.total_files, 2);
        assert_eq!(resp.summary.saved_bytes, u64::MAX, "saturating add, no panic");
    }

    #[test]
    fn query_skipped_lines_passthrough() {
        let lines = vec![start_in("s1", 10, "/out"), "not json".to_string()];
        let resp = query_history(fold_records(&lines), &no_filters(), 0, 50);
        assert_eq!(resp.skipped_lines, 1);
        assert_eq!(resp.total_sessions, 1);
    }

    #[test]
    fn query_blank_and_whitespace_filters_are_inactive() {
        let blank_folder = HistoryFilters { output_folder: Some("  ".into()), ..no_filters() };
        assert_eq!(
            query_history(mixed_folders(), &blank_folder, 0, 50).sessions.len(),
            4,
            "blank output_folder is inactive"
        );

        let blank_status = HistoryFilters { statuses: Some(vec!["  ".into()]), ..no_filters() };
        assert_eq!(
            query_history(two_sessions(), &blank_status, 0, 50).sessions.len(),
            2,
            "blank-only status vec is inactive"
        );

        let mixed = HistoryFilters {
            statuses: Some(vec![" ".into(), "failed".into()]),
            ..no_filters()
        };
        assert_eq!(query_history(two_sessions(), &mixed, 0, 50).sessions.len(), 1);

        let padded = HistoryFilters { statuses: Some(vec![" failed ".into()]), ..no_filters() };
        let resp = query_history(two_sessions(), &padded, 0, 50);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].session_id, "s2");
        assert_eq!(resp.sessions[0].matched_files, Some(1));

        let padded_folder = HistoryFilters { output_folder: Some(" /outB ".into()), ..no_filters() };
        let resp = query_history(two_sessions(), &padded_folder, 0, 50);
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].session_id, "s2");
    }

    #[test]
    fn query_stored_folder_trimmed_for_match_and_listing() {
        let lines = vec![
            start_in("plain", 300, "/outB"),
            start_in("padded", 200, " /outB "),
            start_in("blank", 100, "   "),
        ];
        let fold = fold_records(&lines);

        let unfiltered = query_history(fold, &no_filters(), 0, 50);
        assert_eq!(unfiltered.sessions.len(), 3, "whitespace-only folder still visible");
        assert_eq!(
            unfiltered.output_folders,
            vec!["/outB".to_string()],
            "folders are listed trimmed; whitespace-only is not an option"
        );

        let filtered = query_history(
            fold_records(&lines),
            &HistoryFilters { output_folder: Some("/outB".into()), ..no_filters() },
            0,
            50,
        );
        let ids: Vec<&str> = filtered.sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["plain", "padded"],
            "trimmed filter matches trimmed stored folder on both sides"
        );
        assert_eq!(filtered.total_sessions, 2);
        assert!(
            filtered.sessions.iter().all(|s| s.matched_files.is_none()),
            "folder filter is session-level; matchedFiles stays None"
        );
    }

    #[test]
    fn query_response_serde_keys_are_pinned() {
        let resp = query_history(two_sessions(), &no_filters(), 0, 50);
        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            sorted_keys(&value),
            vec![
                "outputFolders",
                "page",
                "pageSize",
                "sessions",
                "skippedLines",
                "summary",
                "totalFiles",
                "totalSessions",
            ]
        );
        assert_eq!(
            sorted_keys(&value["summary"]),
            vec!["failedFiles", "files", "savedBytes", "sessions"]
        );
        assert_eq!(DEFAULT_PAGE_SIZE, 50);
    }

    #[test]
    fn history_filters_deserialize_camel_case() {
        let filters: HistoryFilters = serde_json::from_str(
            r#"{"sinceMs":5,"outputFolder":"/x","statuses":["failed"],"sort":"oldest"}"#,
        )
        .unwrap();
        assert_eq!(filters.since_ms, Some(5));
        assert_eq!(filters.output_folder, Some("/x".to_string()));
        assert_eq!(filters.statuses, Some(vec!["failed".to_string()]));
        assert_eq!(filters.sort, Some("oldest".to_string()));
        assert_eq!(filters.search, None);
        assert_eq!(filters.categories, None);
        let absent: HistoryFilters = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(absent.sort, None);
        let null: HistoryFilters = serde_json::from_str(r#"{"sort":null}"#).unwrap();
        assert_eq!(null.sort, None);
    }

    #[test]
    fn history_filters_deserialize_rejects_unknown_keys() {
        let result = serde_json::from_str::<HistoryFilters>(r#"{"category":"image"}"#);
        assert!(
            result.is_err(),
            "typo'd filter key must fail deserialization instead of silently defaulting"
        );
    }

    const DAY_MS: i64 = 86_400_000;

    #[test]
    fn trim_keeps_session_exactly_at_boundary() {
        let now = 10 * DAY_MS;
        let lines = vec![
            line(start("old", now - 11 * DAY_MS)),
            line(file("old", "f", now - 11 * DAY_MS + 1, "done", (1, Some(1)))),
            line(start("edge", now - 10 * DAY_MS)),
            line(file("edge", "f", now - 10 * DAY_MS + 1, "done", (1, Some(1)))),
            line(start("fresh", now - 1)),
        ];
        let (kept, dropped) = split_by_retention(lines, 10, now);
        assert_eq!(dropped, 1);
        let out = fold_records(&kept);
        let ids: Vec<&str> = out.sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, vec!["fresh", "edge"]);
    }

    #[test]
    fn trim_newest_exactly_at_cutoff_kept() {
        let now = 10 * DAY_MS;
        let cutoff = now - 10 * DAY_MS;
        let exact = line(start("exact", cutoff));
        let older = line(start("older", cutoff - 1));
        let (kept, dropped) = split_by_retention(vec![exact.clone(), older], 10, now);
        assert_eq!(dropped, 1);
        assert_eq!(kept, vec![exact], "newest == cutoff is kept (strict <)");
    }

    #[test]
    fn trim_ended_at_counts_as_newest_record() {
        let now = 10 * DAY_MS;
        let lines = vec![
            line(start("stale-end", now - 30 * DAY_MS)),
            line(end("stale-end", now - DAY_MS)),
            line(start("fresh-start", now - DAY_MS)),
            line(end("fresh-start", now - 30 * DAY_MS)),
            line(start("stale", now - 30 * DAY_MS)),
            line(end("stale", now - 29 * DAY_MS)),
        ];
        let (kept, dropped) = split_by_retention(lines, 10, now);
        assert_eq!(dropped, 1);
        let out = fold_records(&kept);
        let ids: Vec<&str> = out.sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["fresh-start", "stale-end"],
            "newest is max(started_at, completed_at, ended_at)"
        );
    }

    #[test]
    fn trim_counts_files_after_start_for_interrupted_sessions() {
        let now = 10 * DAY_MS;
        let lines = vec![
            line(start("interrupted-old", now - 20 * DAY_MS)),
            line(file("interrupted-old", "f", now - 2 * DAY_MS, "done", (1, Some(1)))),
        ];
        let (_, dropped) = split_by_retention(lines, 10, now);
        assert_eq!(dropped, 0, "newest record (file) is inside the window");
    }

    #[test]
    fn trim_keeps_malformed_and_orphan_lines() {
        let now = 10 * DAY_MS;
        let lines = vec![
            "not json".to_string(),
            line(file("orphan", "f", now - 99 * DAY_MS, "done", (1, Some(1)))),
        ];
        let (kept, dropped) = split_by_retention(lines, 10, now);
        assert_eq!(dropped, 0);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn trim_keeps_unknown_type_lines_verbatim() {
        let now = 10 * DAY_MS;
        let future = "{\"type\":\"future_record\",\"sessionId\":\"s1\",\"at\":1}".to_string();
        let old_start = line(start("s1", now - 30 * DAY_MS));
        let (kept, dropped) = split_by_retention(vec![future.clone(), old_start], 10, now);
        assert_eq!(dropped, 1);
        assert_eq!(kept, vec![future], "future record kinds survive verbatim");
    }

    #[test]
    fn trim_dropped_session_lines_move_together() {
        let now = 10 * DAY_MS;
        let old_start = line(start("old", now - 30 * DAY_MS));
        let old_file = line(file("old", "f", now - 30 * DAY_MS + 1, "done", (1, Some(1))));
        let old_end = line(end("old", now - 30 * DAY_MS + 2));
        let fresh_start = line(start("fresh", now - 1));
        let lines = vec![old_start, old_file, old_end, fresh_start.clone()];

        let (kept, dropped) = split_by_retention(lines, 10, now);

        assert_eq!(dropped, 1);
        assert_eq!(
            kept,
            vec![fresh_start],
            "dropped session's start, file, and end lines all move together"
        );
    }

    #[test]
    fn trim_orphan_end_before_start_does_not_anchor_session() {
        let now = 100 * DAY_MS;
        let orphan_end = line(end("X", now - 1));
        let old_start_line = line(start("X", now - 30 * DAY_MS));
        let old_file = line(file("X", "f", now - 30 * DAY_MS + 1, "done", (1, Some(1))));
        let lines = vec![orphan_end.clone(), old_start_line, old_file];

        let (kept, dropped) = split_by_retention(lines, 10, now);

        assert_eq!(
            dropped, 1,
            "an orphan end line must not seed newest_by_session; start+file at now-30d decide the drop"
        );
        assert_eq!(
            kept,
            vec![orphan_end],
            "the orphan end line is kept verbatim, its own timestamp ignored"
        );
    }

    #[test]
    fn trim_store_removes_whole_session_lines() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let now = 100 * DAY_MS;
        store
            .append_session_start(&old_start())
            .unwrap();
        store
            .append_file(&FileResultRecord {
                session_id: "old".into(),
                file_id: "f1".into(),
                completed_at: 1,
                name: "a.jpg".into(),
                path: "/in/a.jpg".into(),
                output_path: Some("/o/a.jpg".into()),
                category: "image".into(),
                target_bytes: 1024,
                original_size: 5,
                compressed_size: Some(1),
                status: "done".into(),
                quality_used: Some(70),
                strategy_used: Some("quality=70".into()),
                error: None,
                duration_ms: 12,
                changes: Vec::new(),
                consent_source: None,
            })
            .unwrap();
        store
            .append_session_end(&SessionEndRecord {
                session_id: "old".into(),
                ended_at: 2,
                total_saved_bytes: 4,
                counts: Counts::default(),
                cancelled: false,
            })
            .unwrap();
        let fresh = SessionStartRecord {
            session_id: "fresh".into(),
            started_at: now,
            file_count: 1,
            output_folder: "/o".into(),
        };
        store.append_session_start(&fresh).unwrap();

        let dropped = store.trim(1, now).unwrap();

        assert_eq!(dropped, 1);
        let lines = store.read_lines().unwrap();
        assert_eq!(lines.len(), 1, "exactly one line survives");
        let parsed: SessionStartRecord = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(parsed, fresh);
        assert_eq!(store.load().unwrap().skipped_lines, 0);
    }

    #[test]
    fn trim_wide_retention_does_not_overflow() {
        let now = 10 * DAY_MS;
        let recent = line(start("recent", now));
        let ancient = line(start("ancient", now - 200 * DAY_MS));
        let lines = vec![recent.clone(), ancient.clone()];
        let (kept, dropped) = split_by_retention(lines, u32::MAX, now);
        assert_eq!(dropped, 0);
        assert_eq!(
            kept,
            vec![recent, ancient],
            "cutoff math widens before multiplying; a u32-wrapped cutoff would drop the ancient line"
        );
    }

    #[test]
    fn trim_zero_retention_days_behaves_like_ninety() {
        let now = 100 * DAY_MS;
        let fresh = line(start("fresh", now - 89 * DAY_MS));
        let old = line(start("old", now - 91 * DAY_MS));
        let (kept, dropped) = split_by_retention(vec![fresh.clone(), old], 0, now);
        assert_eq!(dropped, 1);
        assert_eq!(kept, vec![fresh]);
    }

    #[test]
    fn trim_rewrites_only_when_something_drops_and_is_atomic() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store.append_session_start(&old_start()).unwrap();
        let dropped = store.trim(1, 100 * DAY_MS).unwrap();
        assert_eq!(dropped, 1);
        assert!(store.load().unwrap().sessions.is_empty());
        assert!(!dir.path().join("history.jsonl.tmp").exists());

        let after_first_drop = std::fs::read(store.file_path()).unwrap();
        #[cfg(unix)]
        let ino_after_first_drop = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(store.file_path()).unwrap().ino()
        };
        let dropped_again = store.trim(1, 100 * DAY_MS).unwrap();
        assert_eq!(dropped_again, 0);
        assert_eq!(
            std::fs::read(store.file_path()).unwrap(),
            after_first_drop,
            "no-op trim leaves the file byte-identical"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let ino_after_noop = std::fs::metadata(store.file_path()).unwrap().ino();
            assert_eq!(
                ino_after_first_drop, ino_after_noop,
                "no-op trim does not rewrite the file (inode unchanged)"
            );
        }
    }

    #[test]
    fn trim_failure_leaves_original_intact() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("history.jsonl.tmp")).unwrap();
        let store = HistoryStore::new_in(dir.path());
        store.append_session_start(&old_start()).unwrap();
        let before = std::fs::read(store.file_path()).unwrap();
        let err = store.trim(1, 100 * DAY_MS).unwrap_err();
        assert!(
            err.contains("remove stale temp"),
            "tmp occupied by a directory fails at the stale-remove stage: {err}"
        );
        assert_eq!(
            std::fs::read(store.file_path()).unwrap(),
            before,
            "original bytes are byte-identical after a failed rewrite"
        );
    }

    #[test]
    fn trim_read_error_propagates() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        std::fs::create_dir(store.file_path()).unwrap();

        let err = store.trim(1, 100 * DAY_MS).unwrap_err();

        assert!(err.starts_with("history read failed: read "), "{err}");
        assert!(!dir.path().join("history.jsonl.tmp").exists());
    }

    #[test]
    fn trim_direct_write_and_rename_paths_are_used() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let now = 100 * DAY_MS;
        let old = start("old", 0);
        let fresh = start("fresh", now);
        std::fs::write(
            store.file_path(),
            format!("{}\n{}\n", line(old), line(fresh.clone())),
        )
        .unwrap();

        let dropped = store.trim(1, now).unwrap();

        assert_eq!(dropped, 1);
        assert!(
            !dir.path().join("history.jsonl.tmp").exists(),
            "rename consumed the tmp file"
        );
        assert_eq!(
            std::fs::read(store.file_path()).unwrap(),
            format!("{}\n", line(fresh)).into_bytes(),
            "rewrite produced exactly the kept line with a trailing newline"
        );
    }

    #[cfg(unix)]
    #[test]
    fn trim_rewrite_replaces_inode() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let now = 100 * DAY_MS;
        std::fs::write(
            store.file_path(),
            format!(
                "{}\n{}\n",
                line(start("old", 0)),
                line(start("fresh", now))
            ),
        )
        .unwrap();
        let before = std::fs::metadata(store.file_path())
            .unwrap()
            .ino();

        let dropped = store.trim(1, now).unwrap();

        assert_eq!(dropped, 1);
        let after = std::fs::metadata(store.file_path()).unwrap().ino();
        assert_ne!(
            before, after,
            "rename replaces the inode; an in-place truncate-write would keep it"
        );
    }

    #[test]
    fn trim_missing_file_is_noop() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        assert_eq!(store.trim(1, 100 * DAY_MS).unwrap(), 0);
        assert!(!store.file_path().exists());
        assert!(!dir.path().join("history.jsonl.tmp").exists());
    }

    #[test]
    fn trim_stale_tmp_regular_file_is_replaced() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store.append_session_start(&old_start()).unwrap();
        std::fs::write(dir.path().join("history.jsonl.tmp"), "stale").unwrap();
        let dropped = store.trim(1, 100 * DAY_MS).unwrap();
        assert_eq!(dropped, 1);
        assert!(!dir.path().join("history.jsonl.tmp").exists());
        assert!(store.load().unwrap().sessions.is_empty());
    }

    #[test]
    fn clear_removes_file_and_is_idempotent() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store.append_session_end(&small_end("s1")).unwrap();
        let stray = dir.path().join("history.jsonl.tmp");
        std::fs::write(&stray, "stale").unwrap();
        store.clear().unwrap();
        assert!(!store.file_path().exists());
        assert!(!stray.exists());
        store.clear().unwrap();
    }

    #[test]
    fn clear_failure_returns_err() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        std::fs::create_dir_all(store.file_path().join("inner")).unwrap();
        std::fs::write(store.file_path().join("inner").join("data.txt"), "x").unwrap();
        let err = store.clear().unwrap_err();
        assert!(err.starts_with("history clear failed: remove "), "{err}");
        assert!(store.file_path().join("inner").join("data.txt").exists());
    }

    #[test]
    fn clear_tmp_failure_leaves_main_intact() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store.append_session_end(&small_end("s1")).unwrap();
        std::fs::create_dir_all(dir.path().join("history.jsonl.tmp")).unwrap();
        let before = std::fs::read(store.file_path()).unwrap();

        let err = store.clear().unwrap_err();

        assert!(err.starts_with("history clear failed: remove "), "{err}");
        assert!(
            store.file_path().exists(),
            "history.jsonl must survive a tmp removal failure (data left in place)"
        );
        assert_eq!(std::fs::read(store.file_path()).unwrap(), before);
        let lines = store.read_lines().unwrap();
        assert_eq!(lines.len(), 1, "history.jsonl stays readable");
    }

    #[test]
    fn trim_missing_file_cleans_stale_regular_tmp() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let tmp = dir.path().join("history.jsonl.tmp");
        std::fs::write(&tmp, "stale").unwrap();

        assert_eq!(store.trim(1, 100 * DAY_MS).unwrap(), 0);

        assert!(!tmp.exists(), "stale regular tmp is cleaned even with no history file");
        assert!(!store.file_path().exists());
    }

    #[test]
    fn trim_extreme_now_ms_does_not_panic() {
        let now = i64::MIN;
        let lines = vec![line(start("s1", now)), line(start("s2", now + DAY_MS))];
        let (kept, dropped) = split_by_retention(lines.clone(), 90, now);
        assert_eq!(dropped, 0, "cutoff saturates at i64::MIN; nothing is older");
        assert_eq!(kept, lines, "every line survives, in order");
    }

    #[test]
    fn ac2_partial_write_restart_folds_interrupted() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store
            .append_session_start(&SessionStartRecord {
                session_id: "s1".into(),
                started_at: 100,
                file_count: 3,
                output_folder: "/out".into(),
            })
            .unwrap();
        store
            .append_file(&FileResultRecord {
                session_id: "s1".into(),
                file_id: "f1".into(),
                completed_at: 110,
                name: "a.jpg".into(),
                path: "/in/a.jpg".into(),
                output_path: Some("/out/a.jpg".into()),
                category: "image".into(),
                target_bytes: 1024,
                original_size: 500,
                compressed_size: Some(100),
                status: "done".into(),
                quality_used: Some(70),
                strategy_used: Some("quality=70".into()),
                error: None,
                duration_ms: 12,
                changes: Vec::new(),
                consent_source: None,
            })
            .unwrap();
        store
            .append_file(&FileResultRecord {
                session_id: "s1".into(),
                file_id: "f2".into(),
                completed_at: 120,
                name: "b.jpg".into(),
                path: "/in/b.jpg".into(),
                output_path: None,
                category: "image".into(),
                target_bytes: 2048,
                original_size: 300,
                compressed_size: None,
                status: "cancelled".into(),
                quality_used: None,
                strategy_used: None,
                error: None,
                duration_ms: 0,
                changes: Vec::new(),
                consent_source: None,
            })
            .unwrap();

        let out = store.load().unwrap().sessions;
        assert_eq!(out.len(), 1, "partial session is not dropped on restart");
        assert!(out[0].interrupted, "missing session_end folds as Interrupted");
        assert_eq!(out[0].ended_at, None);
        assert_eq!(out[0].files.len(), 2, "only the files that finished recorded");
        assert_eq!(out[0].total_saved_bytes, 400);
        assert_eq!(out[0].outcome.done, 1);
        assert_eq!(out[0].outcome.cancelled, 1);
        assert_eq!(out[0].outcome.failed, 0);
    }

    #[test]
    fn ac3_search_finds_file_beyond_first_page() {
        let mut lines = Vec::new();
        for i in 0..60u32 {
            let sid = format!("s{i:02}");
            let name = format!("file{i:02}.jpg");
            let started = 100 + i64::from(i);
            lines.push(start_in(&sid, started, "/out"));
            lines.push(qfile(
                &sid,
                "f",
                started + 1,
                &name,
                &format!("/in/{name}"),
                Some(&format!("/out/{name}")),
                "image",
                "done",
                (10, Some(1)),
                None,
            ));
            lines.push(line(end(&sid, started + 2)));
        }
        let fold = fold_records(&lines);
        assert_eq!(
            fold.sessions.len(),
            60,
            "60 sessions seeded via in-memory lines"
        );

        let filters = HistoryFilters { search: Some("file00.jpg".into()), ..no_filters() };
        let resp = query_history(fold, &filters, 0, 50);

        assert_eq!(resp.page, 0);
        assert_eq!(
            resp.total_sessions, 1,
            "search covers the entire log, not the loaded page"
        );
        assert_eq!(resp.sessions[0].session_id, "s00");
        assert_eq!(resp.sessions[0].files[0].name, "file00.jpg");
        assert_eq!(resp.sessions[0].matched_files, Some(1));
        assert_eq!(resp.total_files, 1);
        assert_eq!(resp.summary.files, 1, "summary reflects the full filtered set");

        let filters = HistoryFilters { search: Some("file".into()), ..no_filters() };
        let page0 = query_history(fold_records(&lines), &filters, 0, 50);
        assert_eq!(page0.page, 0);
        assert_eq!(
            page0.total_sessions, 60,
            "search covers the entire log, not the loaded page"
        );
        assert_eq!(page0.sessions.len(), 50, "page 0 is a full slice of 50");
        assert_eq!(page0.total_files, 60);

        let page1 = query_history(fold_records(&lines), &filters, 1, 50);
        assert_eq!(page1.page, 1);
        assert_eq!(page1.total_sessions, 60, "totals are page-independent");
        assert_eq!(page1.sessions.len(), 10, "page 1 holds the remaining 10");
        assert_eq!(page1.total_files, 60);
        assert_eq!(page1.summary, page0.summary, "summary is page-independent");
    }

    #[test]
    fn ac5_trim_uses_effective_retention_from_settings() {
        let mut settings = crate::settings::Settings::default();
        settings.history.retention_days = 10;
        let retention = settings.history.effective_retention_days();
        assert_eq!(retention, 10);

        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        let now = 100 * DAY_MS;
        store
            .append_session_start(&SessionStartRecord {
                session_id: "old".into(),
                started_at: now - 11 * DAY_MS,
                file_count: 1,
                output_folder: "/o".into(),
            })
            .unwrap();
        store
            .append_session_start(&SessionStartRecord {
                session_id: "fresh".into(),
                started_at: now - 9 * DAY_MS,
                file_count: 1,
                output_folder: "/o".into(),
            })
            .unwrap();

        let dropped = store.trim(retention, now).unwrap();

        assert_eq!(dropped, 1, "retention of 10 days drops only the old session");
        let out = store.load().unwrap();
        let ids: Vec<&str> = out.sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, vec!["fresh"]);
    }

    #[test]
    fn ac6_mixed_corrupt_file_loads_with_count() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::new_in(dir.path());
        store
            .append_session_start(&SessionStartRecord {
                session_id: "s1".into(),
                started_at: 10,
                file_count: 1,
                output_folder: "/out".into(),
            })
            .unwrap();
        store
            .append_file(&FileResultRecord {
                session_id: "s1".into(),
                file_id: "f1".into(),
                completed_at: 11,
                name: "a.jpg".into(),
                path: "/in/a.jpg".into(),
                output_path: Some("/out/a.jpg".into()),
                category: "image".into(),
                target_bytes: 1024,
                original_size: 5,
                compressed_size: Some(1),
                status: "done".into(),
                quality_used: Some(70),
                strategy_used: Some("quality=70".into()),
                error: None,
                duration_ms: 12,
                changes: Vec::new(),
                consent_source: None,
            })
            .unwrap();
        store
            .append_session_end(&SessionEndRecord {
                session_id: "s1".into(),
                ended_at: 12,
                total_saved_bytes: 4,
                counts: Counts {
                    done: 1,
                    ..Counts::default()
                },
                cancelled: false,
            })
            .unwrap();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(store.file_path())
            .unwrap();
        file.write_all(b"garbage\n").unwrap();
        file.write_all(b"{}\n").unwrap();
        file.write_all(b"[]\n").unwrap();
        file.write_all(
            line(serde_json::json!({ "type": "future_record", "payload": {} })).as_bytes(),
        )
        .unwrap();
        file.write_all(b"\n").unwrap();
        file.write_all(
            b"{\"type\":\"session_end\",\"sessionId\":\"s1\",\"endedAt\":12,\"tota",
        )
        .unwrap();

        let out = store.load().unwrap();
        assert_eq!(
            out.skipped_lines, 4,
            "3 malformed/typeless lines plus the torn tail are skipped; \
             the unknown-type record is still ignored silently"
        );
        assert_eq!(out.sessions.len(), 1, "valid sessions intact");
        assert_eq!(out.sessions[0].session_id, "s1");
        assert_eq!(out.sessions[0].files.len(), 1, "original file record intact");
        assert_eq!(out.sessions[0].outcome.done, 1);
    }

    #[test]
    fn old_history_lines_without_changes_still_load() {
        let mut old_file = file("s1", "f1", 110, "best_effort", (500, Some(100)));
        old_file.as_object_mut().unwrap().remove("changes");
        old_file.as_object_mut().unwrap().remove("consentSource");
        let lines = vec![
            line(start("s1", 100)),
            line(old_file.clone()),
            line(end("s1", 120)),
        ];

        let out = fold_records(&lines);
        assert_eq!(out.skipped_lines, 0, "old file lines must not be skipped");
        assert!(!out.sessions[0].interrupted, "old history must stay complete");
        assert!(out.sessions[0].files[0].changes.is_empty());
        assert_eq!(out.sessions[0].files[0].consent_source, None);

        let record: FileResultRecord = serde_json::from_value(old_file).unwrap();
        assert!(record.changes.is_empty());
        assert_eq!(record.consent_source, None);
    }
}
