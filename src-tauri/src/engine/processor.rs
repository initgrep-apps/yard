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

use crate::engine::progress::ProgressSender;
use crate::engine::result::{CompressionError, CompressionResult};
use crate::models::Category;
use crate::sidecar::{
    GsResources, prepend_engine_dir_to_path, resolve_engine_candidate, resolve_gs_resources,
    resolve_sidecar_candidate, suppress_console_window,
};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

const WAIT_POLL_MILLIS: u64 = 30;
/// Appended to a failing Ghostscript pass when no staged resource tree resolved.
/// `resolve_gs_resources` returning `None` makes `gs_args` omit every `-I` flag,
/// so gs silently falls back to a system install that this build does not ship —
/// the "PDF silently breaks on clean machines" incident. The cause line alone
/// never says that, so the UI-facing error has to.
pub const GS_RESOURCES_MISSING: &str = "the staged gs-resources/ tree is not available, \
    so gs was run with no -I search paths and resolved fonts and ICC profiles from a \
    system Ghostscript this build does not ship (re-run 'yarn binaries:prep' to stage \
    the resources and rebuild)";

/// Wraps a sidecar failure with the missing-resource-tree hint, preserving the
/// original cause so the error still names what actually went wrong.
pub fn gs_failure_with_resources_hint(error: CompressionError) -> CompressionError {
    CompressionError::SidecarFailed(format!("{} — {GS_RESOURCES_MISSING}", error))
}


/// Exhaustive registry of engines a processor may require. Taking this enum
/// instead of a `&str` makes `require` total: there is no stringly-typed
/// wildcard arm left to swallow misspellings at runtime.
///
/// `Ffmpeg`/`Ffprobe` are consumed by the video slice (6) — they register the
/// resolution wiring (`SidecarPaths::resolve_for`) ahead of their processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SidecarName {
    Vips,
    Ffmpeg,
    Ffprobe,
    Gs,
    Qpdf,
}

impl SidecarName {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Vips => "vips",
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
            Self::Gs => "gs",
            Self::Qpdf => "qpdf",
        }
    }
}

/// Resolved sidecar binary paths handed to every processor. Engines whose
/// binary is not present resolve to `None` — each processor enforces the
/// engines it actually needs via `require`, so a missing or future engine
/// (ffmpeg, ffprobe) never breaks unrelated categories. `resolve` swallows
/// IO and non-UTF-8 errors to `None`, so `None` means "not resolved",
/// slightly narrower than every possible failure mode.
#[derive(Debug)]
pub struct SidecarPaths {
    pub vips: Option<String>,
    pub ffmpeg: Option<String>,
    pub ffprobe: Option<String>,
    pub gs: Option<String>,
    pub qpdf: Option<String>,
    /// Staged Ghostscript resource tree. `None` means "not staged": the gs
    /// spawn then carries no `-I` flags, exactly as before the tree existed.
    pub gs_resources: Option<GsResources>,
}

impl SidecarPaths {
    pub fn resolve_for(exe_dir: &Path) -> SidecarPaths {
        let resolve = |name: &str| resolve_sidecar_candidate(exe_dir, name).ok();
        // vips/gs/qpdf need their sibling DLLs, so on Windows they live in
        // per-engine folders; ffmpeg/ffprobe stay flat beside the app binary.
        let resolve_engine = |name: &str| resolve_engine_candidate(exe_dir, name).ok();
        SidecarPaths {
            vips: resolve_engine("vips").or_else(|| resolve("vips")),
            ffmpeg: resolve("ffmpeg"),
            ffprobe: resolve("ffprobe"),
            gs: resolve_engine("gs").or_else(|| resolve("gs")),
            qpdf: resolve_engine("qpdf").or_else(|| resolve("qpdf")),
            gs_resources: resolve_gs_resources(exe_dir),
        }
    }

    pub fn require(&self, name: SidecarName) -> Result<&str, CompressionError> {
        let resolved = match name {
            SidecarName::Vips => &self.vips,
            SidecarName::Ffmpeg => &self.ffmpeg,
            SidecarName::Ffprobe => &self.ffprobe,
            SidecarName::Gs => &self.gs,
            SidecarName::Qpdf => &self.qpdf,
        };
        resolved.as_deref().ok_or_else(|| {
            CompressionError::SidecarNotFound(name.as_str().to_string())
        })
    }
}

/// The best attempt seen so far in a multi-pass search. `size` is the byte
/// length of the output at `path`; `strategy`/`quality` describe the pass
/// that produced it. `Ladder::record` is the only caller of
/// `keep_smaller_in`, so every processor shares one smallest-wins rule.
#[derive(Debug, Clone)]
pub struct BestAttempt {
    pub size: u64,
    pub path: PathBuf,
    pub strategy: String,
    pub quality: u32,
}

impl BestAttempt {
    /// Records `candidate` into `best` only when it is strictly smaller
    /// (ties keep the earlier attempt). Returns true when replaced.
    pub fn keep_smaller_in(best: &mut Option<BestAttempt>, candidate: BestAttempt) -> bool {
        let replaces = best.as_ref().map(|b| candidate.size < b.size).unwrap_or(true);
        if replaces {
            *best = Some(candidate);
        }
        replaces
    }
}

const STDERR_TAIL_CHARS: usize = 500;
const STDERR_TAIL_KEEP_BYTES: usize = 2048;
const STDERR_DRAIN_CHUNK: usize = 4096;
const DELIVER_COPY_CHUNK: usize = 65536;
const STDOUT_CAPTURE_LIMIT: usize = 64 * 1024;

/// Copies `from` to the final output path and reports completion. One
/// enforcement point for the copy + send_progress(100) + result triple
/// shared by every processor's deliverable paths (early exit, fitting pass,
/// best-effort).
pub fn deliver_output(
    from: &Path,
    output: &Path,
    size: u64,
    strategy_used: String,
    quality_used: u32,
    met_target: bool,
    progress: &ProgressSender,
    cancel_token: &CancellationToken,
) -> Result<CompressionResult, CompressionError> {
    if from == output {
        progress.send_progress(100);
        return Ok(CompressionResult {
            output_path: output.to_path_buf(),
            compressed_size: size,
            quality_used,
            met_target,
            strategy_used,
        });
    }

    let temp_path = output.with_extension("yard-tmp");
    if let Err(e) = std::fs::remove_file(&temp_path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!("warn: could not remove stale temp {}: {e}", temp_path.display());
        }
    }

    let mut src = std::fs::File::open(from)
        .map_err(|e| CompressionError::OutputFileError(e.to_string()))?;
    let mut dst = std::fs::File::create(&temp_path)
        .map_err(|e| CompressionError::OutputFileError(e.to_string()))?;

    let mut buf = [0u8; DELIVER_COPY_CHUNK];
    loop {
        if cancel_token.is_cancelled() {
            let _ = std::fs::remove_file(&temp_path);
            return Err(CompressionError::Cancelled);
        }
        match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(e) = dst.write_all(&buf[..n]) {
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(CompressionError::OutputFileError(e.to_string()));
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&temp_path);
                return Err(CompressionError::OutputFileError(e.to_string()));
            }
        }
    }

    if cancel_token.is_cancelled() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(CompressionError::Cancelled);
    }

    drop(dst);
    if let Err(e) = commit_output(&temp_path, output) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(CompressionError::OutputFileError(e.to_string()));
    }

    progress.send_progress(100);
    Ok(CompressionResult {
        output_path: output.to_path_buf(),
        compressed_size: size,
        quality_used,
        met_target,
        strategy_used,
    })
}

/// Atomically commits a temp file to its final destination. `hard_link`
/// fails if `dest` exists on both Unix and Windows, so the temp stays
/// reachable and the caller can clean it up. On filesystems that do not
/// support hard links (exFAT/FAT, cross-device), fall back to the previous
/// checked-rename path.
pub fn commit_output(temp: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::hard_link(temp, dest) {
        Ok(()) => {
            if let Err(e) = std::fs::remove_file(temp) {
                // shortcut: temp cleanup best-effort only; a stuck temp file
                // is harmless but should be logged. Upgrade if we ever need
                // strict temp accounting.
                eprintln!("warn: could not remove committed temp {}: {e}", temp.display());
            }
            Ok(())
        }
        Err(e) if link_unsupported(&e) => {
            // shortcut: checked rename leaves a residual race on filesystems
            // without hard-link support; production bundles target APFS/NTFS.
            if dest.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("destination {} already exists", dest.display()),
                ));
            }
            std::fs::rename(temp, dest)
        }
        Err(e) => Err(e),
    }
}

fn link_unsupported(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::Unsupported
            | std::io::ErrorKind::CrossesDevices
            | std::io::ErrorKind::PermissionDenied
            | std::io::ErrorKind::InvalidInput
    )
}

/// Bounded stderr tail: retains the last ~2 KiB of a sidecar stream while
/// still accounting for every byte that passed through, so callers can report
/// diagnostics without unbounded memory growth.
pub struct StderrTail {
    buffer: Vec<u8>,
    bytes_seen: u64,
}

impl Default for StderrTail {
    fn default() -> Self {
        Self::new()
    }
}

impl StderrTail {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(STDERR_TAIL_KEEP_BYTES),
            bytes_seen: 0,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.bytes_seen += chunk.len() as u64;
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() > STDERR_TAIL_KEEP_BYTES {
            let drop = self.buffer.len() - STDERR_TAIL_KEEP_BYTES;
            self.buffer.drain(..drop);
        }
    }

    pub fn bytes_seen(&self) -> u64 {
        self.bytes_seen
    }

    pub fn tail(&self) -> &[u8] {
        &self.buffer
    }
}

/// Last ~`STDERR_TAIL_CHARS` characters of captured stderr — the failure
/// reason is almost always the last thing printed.
pub fn stderr_tail(tail: &StderrTail) -> String {
    let stderr = String::from_utf8_lossy(tail.tail()).into_owned();
    let tail: String = {
        let count = stderr.chars().count();
        if count > STDERR_TAIL_CHARS {
            stderr.chars().skip(count - STDERR_TAIL_CHARS).collect()
        } else {
            stderr
        }
    };
    let detail = tail.trim_end();
    if detail.is_empty() {
        "<no stderr>".to_string()
    } else {
        detail.to_string()
    }
}

/// A sidecar exit code that, for this engine, still counts as pass success
/// and leaves output validation to the caller (e.g. qpdf exit 2 = "warnings,
/// output still valid"). Exit-code policy is per-engine, so the engine layer
/// takes it as a predicate instead of hardcoding tool quirks.
pub type ToleratedExit = fn(&str, Option<i32>) -> bool;

/// No tolerated exit codes: any non-zero exit fails the pass. Default for
/// engines whose tools have no "warning" exit code (vips, gs, ffmpeg).
pub fn strict_exit(_label: &str, _code: Option<i32>) -> bool {
    false
}

/// Runs a sidecar to completion, draining stderr on a reader thread so a
/// chatty tool can never deadlock on a full pipe. Polls the cancel token:
/// a cancel SIGKILLs the child process first, then surfaces as `Cancelled`
/// — cancelled beats exit code. The drain thread uses a shared `StderrTail`
/// so the same bounded retention applies to every sidecar path.
pub fn run_sidecar(
    sidecar_path: &str,
    label: &'static str,
    args: &[String],
    cancel_token: &CancellationToken,
    tolerated_exit: ToleratedExit,
) -> Result<(), CompressionError> {
    run_sidecar_inner(
        sidecar_path,
        label,
        args,
        cancel_token,
        tolerated_exit,
        false,
    )
    .map(|_| ())
}

/// Runs a sidecar and captures its stdout (up to 64 KiB) for callers that
/// need the tool's answer, such as `qpdf --show-npages`. stderr keeps the
/// same bounded-tail contract as `run_sidecar`.
pub fn run_sidecar_capture_stdout(
    sidecar_path: &str,
    label: &'static str,
    args: &[String],
    cancel_token: &CancellationToken,
    tolerated_exit: ToleratedExit,
) -> Result<(std::process::ExitStatus, String), CompressionError> {
    let (status, stdout) = run_sidecar_inner(
        sidecar_path,
        label,
        args,
        cancel_token,
        tolerated_exit,
        true,
    )?;
    Ok((status, String::from_utf8_lossy(&stdout).into_owned()))
}

fn run_sidecar_inner(
    sidecar_path: &str,
    label: &'static str,
    args: &[String],
    cancel_token: &CancellationToken,
    tolerated_exit: ToleratedExit,
    capture_stdout: bool,
) -> Result<(std::process::ExitStatus, Vec<u8>), CompressionError> {
    let mut command = std::process::Command::new(sidecar_path);
    command.args(args).stderr(std::process::Stdio::piped());
    if capture_stdout {
        command.stdout(std::process::Stdio::piped());
    } else {
        command.stdout(std::process::Stdio::null());
    }
    suppress_console_window(&mut command);
    prepend_engine_dir_to_path(&mut command, sidecar_path);
    let mut child = command.spawn().map_err(|e| {
            CompressionError::SidecarFailed(format!("Failed to start {}: {}", label, e))
        })?;

    let mut stderr_pipe = child.stderr.take();
    let stderr_reader = std::thread::spawn(move || {
        let mut tail = StderrTail::new();
        if let Some(pipe) = stderr_pipe.as_mut() {
            let mut buf = [0u8; STDERR_DRAIN_CHUNK];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => tail.push(&buf[..n]),
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        eprintln!("warn: {label}: stderr drain failed, diagnostics partial: {e}");
                        break;
                    }
                }
            }
        }
        tail
    });

    let stdout_reader = child
        .stdout
        .take()
        .map(|pipe| std::thread::spawn(move || capture_bounded_stdout(pipe, label)));

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if cancel_token.is_cancelled() {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CompressionError::Cancelled);
                }
                std::thread::sleep(std::time::Duration::from_millis(WAIT_POLL_MILLIS));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CompressionError::SidecarFailed(format!(
                    "{} wait failed: {}",
                    label, e
                )));
            }
        }
    };

    if cancel_token.is_cancelled() {
        return Err(CompressionError::Cancelled);
    }
    let tail = match stderr_reader.join() {
        Ok(tail) => tail,
        Err(_) => {
            eprintln!("warn: {label} stderr drain thread panicked");
            StderrTail::new()
        }
    };
    let stdout = match stdout_reader {
        Some(reader) => match reader.join() {
            Ok(stdout) => stdout,
            Err(_) => {
                eprintln!("warn: {label} stdout capture thread panicked");
                Vec::new()
            }
        },
        None => Vec::new(),
    };

    if !status.success() && !tolerated_exit(label, status.code()) {
        if cancel_token.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }
        return Err(CompressionError::SidecarFailed(format!(
            "{} exited with code {:?}: {}",
            label,
            status.code(),
            stderr_tail(&tail)
        )));
    }
    Ok((status, stdout))
}

fn capture_bounded_stdout(mut pipe: impl Read, label: &str) -> Vec<u8> {
    let mut captured = Vec::new();
    let mut buf = [0u8; STDERR_DRAIN_CHUNK];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let remaining = STDOUT_CAPTURE_LIMIT.saturating_sub(captured.len());
                captured.extend_from_slice(&buf[..n.min(remaining)]);
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                eprintln!("warn: {label}: stdout capture failed, answer partial: {e}");
                break;
            }
        }
    }
    captured
}

pub trait CompressionProcessor: Send + Sync {
    fn category(&self) -> Category;
    fn supported_extensions(&self) -> &[&str];
    fn compress(
        &self,
        ctx: &crate::engine::context::JobContext<'_>,
    ) -> Result<CompressionResult, CompressionError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(windows)]
    #[test]
    fn run_sidecar_prepends_engine_dir_on_windows() {
        let dir = tempfile::TempDir::new().unwrap();
        let engine = dir.path().join("engines").join("vips");
        std::fs::create_dir_all(&engine).unwrap();
        let exe = engine.join("vips.exe");
        std::fs::write(&exe, b"stub").unwrap();

        let mut command = std::process::Command::new("does-not-run");
        let applied = prepend_engine_dir_to_path(&mut command, exe.to_str().unwrap());

        let applied = applied.expect("Windows engine folders must get a PATH prepend");
        assert!(
            applied
                .to_string_lossy()
                .starts_with(engine.to_string_lossy().as_ref()),
            "the engine's own folder must come first so its DLLs win: {}",
            applied.to_string_lossy()
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn run_sidecar_leaves_path_alone_off_windows() {
        let mut command = std::process::Command::new("does-not-run");
        assert!(
            prepend_engine_dir_to_path(&mut command, "/opt/engine/vips").is_none(),
            "macOS/Linux resolve rpaths at load time, so a PATH prepend would be a silent no-op \
             that hides the fact the wiring is untested there"
        );
    }

    #[test]
    fn resolve_for_reads_engines_from_the_engine_folder_layout() {
        let dir = tempfile::TempDir::new().unwrap();
        let exe_dir = dir.path().join("app");
        std::fs::create_dir_all(&exe_dir).unwrap();
        for name in ["vips", "gs", "qpdf"] {
            let engine = exe_dir.join("resources").join("engines").join(name);
            std::fs::create_dir_all(&engine).unwrap();
            std::fs::write(engine.join(format!("{name}.exe")), b"stub").unwrap();
        }

        let paths = SidecarPaths::resolve_for(&exe_dir);

        assert!(paths.vips.is_some(), "vips must resolve from engines/vips/");
        assert!(paths.gs.is_some(), "gs must resolve from engines/gs/");
        assert!(
            paths.qpdf.is_some(),
            "qpdf must resolve from engines/qpdf/, which is where its VC runtime DLLs live"
        );
    }

    #[test]
    fn resolve_for_finds_flat_stripped_exe_names_and_falls_back_without_an_engine_folder() {
        let dir = tempfile::TempDir::new().unwrap();
        let exe_dir = dir.path().join("app");
        std::fs::create_dir_all(&exe_dir).unwrap();
        for name in ["ffmpeg", "ffprobe", "qpdf"] {
            std::fs::write(exe_dir.join(format!("{name}.exe")), b"stub").unwrap();
        }

        let paths = SidecarPaths::resolve_for(&exe_dir);

        assert!(
            paths.ffmpeg.is_some(),
            "ffmpeg ships as a stripped-name externalBin beside the app binary, not in an engine \
             folder"
        );
        assert!(paths.ffprobe.is_some());
        assert!(
            paths.qpdf.is_some(),
            "qpdf must still resolve from the flat stripped-name exe-dir candidate when no engine \
             folder exists, so existing layouts keep working"
        );
    }

    #[test]
    fn resolve_for_finds_every_engine_in_a_bundled_package() {
        let exe_dir = match std::env::var("FSO_BUNDLE_EXE_DIR") {
            Ok(dir) => dir,
            Err(_) => {
                assert!(
                    std::env::var_os("FSO_BUNDLE_GATE").is_none(),
                    "FSO_BUNDLE_GATE is set but FSO_BUNDLE_EXE_DIR is missing — the package gate must \
                     pass the built package's executable directory"
                );
                eprintln!(
                    "skipping resolve_for_finds_every_engine_in_a_bundled_package: FSO_BUNDLE_EXE_DIR is \
                     not set — the package gate sets it to the built package's executable directory"
                );
                return;
            }
        };
        let exe_dir = std::path::PathBuf::from(exe_dir);
        assert!(
            exe_dir.is_absolute(),
            "FSO_BUNDLE_EXE_DIR must be absolute, got {exe_dir:?} — the package gate passes the built \
             package's executable directory"
        );
        assert!(
            exe_dir.is_dir(),
            "FSO_BUNDLE_EXE_DIR must be a directory, got {exe_dir:?} — the package gate passes the built \
             package's executable directory"
        );
        let paths = SidecarPaths::resolve_for(&exe_dir);
        for (name, resolved) in [
            ("vips", &paths.vips),
            ("ffmpeg", &paths.ffmpeg),
            ("ffprobe", &paths.ffprobe),
            ("gs", &paths.gs),
            ("qpdf", &paths.qpdf),
        ] {
            let resolved = resolved.as_deref().unwrap_or_else(|| {
                panic!(
                    "{name} did not resolve from the built package at {exe_dir:?} — the app would \
                     report 'Engine error: {name} not available' at compression time"
                )
            });
            assert!(
                !std::path::Path::new(resolved)
                    .components()
                    .any(|c| c == std::path::Component::ParentDir),
                "{name} resolved to {resolved:?}, which climbs out of {exe_dir:?} — the dev-tree \
                 fallback satisfied this, so the package itself may not ship {name}"
            );
            assert!(
                std::path::Path::new(resolved).starts_with(&exe_dir),
                "{name} resolved to {resolved:?}, outside {exe_dir:?}"
            );
            let file_name = std::path::Path::new(resolved)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_else(|| panic!("{name} resolved to {resolved:?}, which has no file name"));
            let stem = file_name.strip_suffix(".exe").unwrap_or(file_name);
            assert!(
                stem == name,
                "{name} resolved to {resolved:?} — packaged engines use their plain name"
            );
        }
        #[cfg(not(target_os = "windows"))]
        if std::env::var_os("FSO_BUNDLE_GATE").is_some() {
            assert!(
                paths.gs_resources.is_some(),
                "gs resources did not resolve from the built package at {exe_dir:?} — gs would fall \
                 back to its compiled-in prefix and PDF compression would fail on a machine without \
                 Ghostscript"
            );
        }
        println!("fso-bundle-resolver-ok: {exe_dir:?}");
    }

    fn empty_paths() -> SidecarPaths {
        SidecarPaths {
            vips: None,
            ffmpeg: None,
            ffprobe: None,
            gs: None,
            qpdf: None,
            gs_resources: None,
        }
    }

    #[test]
    fn require_maps_every_sidecar_name_to_its_field() {
        let paths = SidecarPaths {
            vips: Some("/bin/vips".to_string()),
            ffmpeg: Some("/bin/ffmpeg".to_string()),
            ffprobe: Some("/bin/ffprobe".to_string()),
            gs: Some("/bin/gs".to_string()),
            qpdf: Some("/bin/qpdf".to_string()),
            gs_resources: None,
        };
        for (name, expected) in [
            (SidecarName::Vips, "/bin/vips"),
            (SidecarName::Ffmpeg, "/bin/ffmpeg"),
            (SidecarName::Ffprobe, "/bin/ffprobe"),
            (SidecarName::Gs, "/bin/gs"),
            (SidecarName::Qpdf, "/bin/qpdf"),
        ] {
            assert_eq!(paths.require(name).unwrap(), expected, "name: {name:?}");
        }
    }

    #[test]
    fn require_returns_sidecar_not_found_for_unset_engine() {
        let paths = empty_paths();
        let err = paths.require(SidecarName::Gs).unwrap_err();
        assert!(
            matches!(err, CompressionError::SidecarNotFound(ref n) if n == "gs"),
            "got: {:?}",
            err
        );
        assert_eq!(err.to_string(), "Engine error: gs not available");
    }

    #[test]
    fn sidecar_name_as_str_covers_all_variants() {
        let names = [
            SidecarName::Vips,
            SidecarName::Ffmpeg,
            SidecarName::Ffprobe,
            SidecarName::Gs,
            SidecarName::Qpdf,
        ];
        for name in names {
            match name {
                SidecarName::Vips => assert_eq!(name.as_str(), "vips"),
                SidecarName::Ffmpeg => assert_eq!(name.as_str(), "ffmpeg"),
                SidecarName::Ffprobe => assert_eq!(name.as_str(), "ffprobe"),
                SidecarName::Gs => assert_eq!(name.as_str(), "gs"),
                SidecarName::Qpdf => assert_eq!(name.as_str(), "qpdf"),
            }
        }
    }

    #[test]
    fn best_attempt_keep_smaller_in_keeps_strictly_smallest() {
        let mut best: Option<BestAttempt> = None;

        assert!(BestAttempt::keep_smaller_in(
            &mut best,
            BestAttempt {
                size: 500,
                path: PathBuf::from("a"),
                strategy: "qpdf-lossless".to_string(),
                quality: 100,
            },
        ));
        let first = best.as_ref().unwrap();
        assert_eq!(first.size, 500);
        assert_eq!(first.path, PathBuf::from("a"));

        assert!(!BestAttempt::keep_smaller_in(
            &mut best,
            BestAttempt {
                size: 500,
                path: PathBuf::from("tie"),
                strategy: "tie".to_string(),
                quality: 1,
            },
        ));
        assert_eq!(best.as_ref().unwrap().path, PathBuf::from("a"));

        assert!(BestAttempt::keep_smaller_in(
            &mut best,
            BestAttempt {
                size: 400,
                path: PathBuf::from("b"),
                strategy: "gs dpi=72,jpegq=40".to_string(),
                quality: 40,
            },
        ));
        let winner: &BestAttempt = best.as_ref().unwrap();
        assert_eq!(winner.size, 400);
        assert_eq!(winner.path, PathBuf::from("b"));
        assert_eq!(winner.quality, 40);
    }

    #[test]
    fn cancelled_copy_leaves_no_final_file() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.bin");
        let output = dir.path().join("out.bin");
        let temp_path = output.with_extension("yard-tmp");
        let size = 64 * 1024 * 1024;
        fs::write(&input, vec![0u8; size]).unwrap();
        fs::write(&temp_path, b"stale").unwrap();

        let token = CancellationToken::new();
        let worker_token = token.clone();
        let worker_input = input.clone();
        let worker_output = output.clone();
        let handle = std::thread::spawn(move || {
            deliver_output(
                &worker_input,
                &worker_output,
                size as u64,
                "test".to_string(),
                75,
                true,
                &ProgressSender::test_stub(),
                &worker_token,
            )
        });

        let mut triggered = false;
        for _ in 0..10000 {
            if temp_path.exists()
                && fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0) > 0
            {
                triggered = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(triggered, "temp file was never created");
        token.cancel();
        let result = handle.join().unwrap();
        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "got: {:?}",
            result
        );
        assert!(!output.exists(), "final output must not exist");
        assert!(!temp_path.exists(), "temp file must be cleaned");
    }

    #[test]
    #[cfg(unix)]
    fn failed_copy_removes_temp_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.bin");
        fs::write(&input, b"x").unwrap();
        let output_dir = dir.path().join("readonly");
        fs::create_dir(&output_dir).unwrap();
        fs::set_permissions(&output_dir, fs::Permissions::from_mode(0o500)).unwrap();
        let output = output_dir.join("out.bin");

        let result = deliver_output(
            &input,
            &output,
            1,
            "test".to_string(),
            75,
            true,
            &ProgressSender::test_stub(),
            &CancellationToken::new(),
        );

        assert!(
            matches!(result, Err(CompressionError::OutputFileError(_))),
            "got: {:?}",
            result
        );
        assert!(
            !output.with_extension("yard-tmp").exists(),
            "temp file must not be left in the read-only directory"
        );

        fs::set_permissions(&output_dir,
            fs::Permissions::from_mode(0o755)
        ).unwrap();
    }

    #[test]
    fn existing_destination_is_not_clobbered() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.bin");
        fs::write(&input, b"new content").unwrap();
        let output = dir.path().join("out.bin");
        fs::write(&output, b"precious existing data").unwrap();

        let result = deliver_output(
            &input,
            &output,
            11,
            "test".to_string(),
            75,
            true,
            &ProgressSender::test_stub(),
            &CancellationToken::new(),
        );

        assert!(
            matches!(result, Err(CompressionError::OutputFileError(_))),
            "got: {:?}",
            result
        );
        assert!(!output.with_extension("yard-tmp").exists(), "temp must be removed");
        assert_eq!(
            fs::read(&output).unwrap(),
            b"precious existing data",
            "existing destination must not be clobbered"
        );
        assert!(!dir.path().join("out.bin.yard-tmp").exists());
    }

    #[test]
    fn successful_commit_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.bin");
        fs::write(&input, b"delivered").unwrap();
        let output = dir.path().join("out.bin");

        let result = deliver_output(
            &input,
            &output,
            9,
            "test".to_string(),
            75,
            true,
            &ProgressSender::test_stub(),
            &CancellationToken::new(),
        );

        assert!(result.is_ok(), "got: {:?}", result);
        assert!(output.exists(), "destination must exist");
        assert_eq!(fs::read(&output).unwrap(), b"delivered");
        assert!(
            !output.with_extension("yard-tmp").exists(),
            "temp must be removed after successful commit"
        );
    }

    #[test]
    fn stderr_tail_retains_at_most_2_kib_but_drains_everything() {
        let mut tail = StderrTail::new();
        let total = 10 * 1024 * 1024;
        let chunk_size = 4096;
        let chunk = vec![b'x'; chunk_size];
        for _ in 0..(total / chunk_size) {
            tail.push(&chunk);
        }
        assert_eq!(tail.bytes_seen(), total as u64, "every byte must be accounted for");
        assert!(
            tail.tail().len() <= STDERR_TAIL_KEEP_BYTES,
            "retained tail {} exceeds {STDERR_TAIL_KEEP_BYTES} bytes",
            tail.tail().len()
        );
    }

    #[cfg(unix)]
    #[test]
    fn capture_stdout_is_capped_and_stderr_stays_bounded() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let chatty = dir.path().join("chatty");
        fs::write(
            &chatty,
            r#"#!/bin/bash
head -c 100000 /dev/zero | tr '\0' 'x'
printf 'CAPTURE_TAIL\n' >&2
"#,
        )
        .unwrap();
        fs::set_permissions(&chatty, fs::Permissions::from_mode(0o755)).unwrap();

        let (status, stdout) = run_sidecar_capture_stdout(
            chatty.to_str().unwrap(),
            "qpdf",
            &[],
            &CancellationToken::new(),
            strict_exit,
        )
        .unwrap();

        assert!(status.success());
        assert_eq!(
            stdout.len(),
            STDOUT_CAPTURE_LIMIT,
            "stdout must be capped at 64 KiB but fully drained"
        );
        assert!(stdout.bytes().all(|b| b == b'x'));
    }

    #[cfg(unix)]
    #[test]
    fn capture_stdout_failure_still_reports_the_stderr_tail() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let failing = dir.path().join("failing");
        fs::write(
            &failing,
            r#"#!/bin/bash
printf 'STDOUT_ANSWER\n'
printf 'STDERR_CAUSE\n' >&2
exit 3
"#,
        )
        .unwrap();
        fs::set_permissions(&failing, fs::Permissions::from_mode(0o755)).unwrap();

        match run_sidecar_capture_stdout(
            failing.to_str().unwrap(),
            "qpdf",
            &[],
            &CancellationToken::new(),
            strict_exit,
        ) {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(msg.contains("exited with code Some(3)"), "msg: {msg}");
                assert!(msg.contains("STDERR_CAUSE"), "msg: {msg}");
            }
            other => panic!("expected SidecarFailed, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn release_profile_keeps_unwind_panics() {
        let manifest = fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
        ).unwrap();
        let release = manifest
            .split("[profile.release]")
            .nth(1)
            .expect("release profile must exist");
        assert!(
            release.contains("panic = \"unwind\""),
            "release profile must keep panic = unwind"
        );
    }
}
