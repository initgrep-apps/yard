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

use std::path::{Path, PathBuf};

use crate::engine::context::JobContext;
use crate::engine::processor::{BestAttempt, deliver_output};
use crate::engine::progress::ProgressSender;
use crate::engine::result::{CompressionError, CompressionResult, STRATEGY_ALREADY_UNDER_TARGET};
use crate::policy::{Change, ConsentSource};

/// The single pre-delivery progress scale. Attempts report their own 0..=100
/// position; the ladder maps that once into this ceiling and the terminal 100
/// is emitted by delivery.
pub const LADDER_PROGRESS_MAX: u32 = 90;

#[derive(Debug)]
pub enum AttemptOutcome {
    Produced {
        path: PathBuf,
        size: u64,
        quality: u32,
        strategy: String,
        changes: Vec<Change>,
        consent: ConsentSource,
    },
    Failed(CompressionError),
}

#[derive(Debug)]
pub enum AttemptDecision {
    Continue,
    Stop(CompressionResult),
}

/// Owns every cross-attempt concern: the shortest-fit copy, best tracking,
/// cancellation cleanup, attempt-file removal and the best-effort tail.
pub struct Ladder {
    best: Option<BestAttempt>,
    attempts: Vec<PathBuf>,
    last_error: Option<CompressionError>,
    permission_denial: Option<String>,
    cancelled: bool,
    progress: Option<ProgressSender>,
}

impl Default for Ladder {
    fn default() -> Self {
        Self::new()
    }
}

impl Ladder {
    pub fn new() -> Self {
        Ladder {
            best: None,
            attempts: Vec::new(),
            last_error: None,
            permission_denial: None,
            cancelled: false,
            progress: None,
        }
    }

    /// The one pre-delivery scale shared by every attempt. The sender is
    /// monotonic across attempts, so a new attempt can never regress the bar.
    pub fn progress(&mut self, ctx: &JobContext<'_>) -> ProgressSender {
        self.progress
            .get_or_insert_with(|| ctx.progress.scaled(LADDER_PROGRESS_MAX))
            .clone()
    }

    /// Registers an attempt file before running its engine so cancellation
    /// and failure can remove it even when the attempt never returns.
    pub fn track(&mut self, path: &Path) {
        if !self.attempts.iter().any(|attempt| attempt == path) {
            self.attempts.push(path.to_path_buf());
        }
    }

    /// Copies an already-fitting input through the atomic commit path. `None`
    /// means the input is over target; delivery failures are propagated so an
    /// under-target job cannot silently fall into the compression pipeline.
    pub fn shortcut(
        &self,
        ctx: &JobContext<'_>,
        input_size: u64,
    ) -> Result<Option<CompressionResult>, CompressionError> {
        if input_size > ctx.target_bytes {
            return Ok(None);
        }
        deliver_output(
            ctx.input,
            ctx.output,
            input_size,
            STRATEGY_ALREADY_UNDER_TARGET.to_string(),
            100,
            true,
            &ctx.progress,
            &ctx.cancel,
        )
        .map(Some)
    }

    pub fn record(&mut self, ctx: &JobContext<'_>, outcome: AttemptOutcome) -> AttemptDecision {
        if let AttemptOutcome::Produced { path, .. } = &outcome {
            self.track(path);
        }
        if ctx.cancel.is_cancelled() {
            self.cancel_cleanup();
            return AttemptDecision::Continue;
        }

        match outcome {
            AttemptOutcome::Produced {
                path,
                size,
                quality,
                strategy,
                changes,
                consent,
            } => {
                if let Err(error) = Self::check_declared(ctx.declared, &changes) {
                    self.remove_attempt(&path);
                    self.last_error = Some(CompressionError::Unknown(error));
                    return AttemptDecision::Continue;
                }
                if ctx.policy.check(ctx.category, &changes) != crate::policy::Verdict::Allowed {
                    self.remove_attempt(&path);
                    self.permission_denial = Some(
                        ctx.policy
                            .denial_reason(ctx.category, &changes)
                            .unwrap_or_else(|| "Needs permission".to_string()),
                    );
                    return AttemptDecision::Continue;
                }
                let attempt = BestAttempt {
                    size,
                    path,
                    strategy,
                    quality,
                    changes,
                    consent,
                };
                if size > ctx.target_bytes {
                    BestAttempt::keep_smaller_in(&mut self.best, attempt);
                    if ctx.cancel.is_cancelled() {
                        self.cancel_cleanup();
                    }
                    return AttemptDecision::Continue;
                }

                let result = deliver_output(
                    &attempt.path,
                    ctx.output,
                    attempt.size,
                    attempt.strategy.clone(),
                    attempt.quality,
                    true,
                    &ctx.progress,
                    &ctx.cancel,
                );
                match result {
                    Ok(mut result) => {
                        result.changes = attempt.changes;
                        result.consent = attempt.consent;
                        self.permission_denial = None;
                        self.best = None;
                        self.cleanup_attempts();
                        AttemptDecision::Stop(result)
                    }
                    Err(error) => {
                        self.best = None;
                        self.cleanup_attempts();
                        self.last_error = Some(error);
                        AttemptDecision::Continue
                    }
                }
            }
            AttemptOutcome::Failed(error) => {
                self.remove_last_attempt();
                if matches!(error, CompressionError::Cancelled) {
                    self.cancel_cleanup();
                } else {
                    self.last_error = Some(error);
                    if ctx.cancel.is_cancelled() {
                        self.cancel_cleanup();
                    }
                }
                AttemptDecision::Continue
            }
        }
    }

    pub fn check_declared(declared: &[Change], changes: &[Change]) -> Result<(), String> {
        match changes
            .iter()
            .find(|change| !declared.contains(change))
        {
            Some(change) => Err(format!(
                "undeclared change: {} was not declared by this rung",
                change.as_str()
            )),
            None => Ok(()),
        }
    }

    /// Delivers the best over-target attempt as best-effort, or surfaces the
    /// last attempt error. Consumes the ladder so attempt files cannot leak.
    pub fn finish(mut self, ctx: &JobContext<'_>) -> Result<CompressionResult, CompressionError> {
        if self.cancelled || ctx.cancel.is_cancelled() {
            self.cleanup_attempts();
            return Err(CompressionError::Cancelled);
        }
        if let Some(reason) = self.permission_denial.take() {
            self.cleanup_attempts();
            return Err(CompressionError::NeedsPermission(reason));
        }
        let Some(best) = self.best.take() else {
            self.cleanup_attempts();
            return Err(self
                .last_error
                .take()
                .unwrap_or_else(|| CompressionError::SidecarFailed("Could not compress file".into())));
        };
        if let Some(reason) = ctx.policy.denial_reason(ctx.category, &best.changes) {
            self.cleanup_attempts();
            return Err(CompressionError::NeedsPermission(reason));
        }
        let result = deliver_output(
            &best.path,
            ctx.output,
            best.size,
            format!("{},best-effort", best.strategy),
            best.quality,
            false,
            &ctx.progress,
            &ctx.cancel,
        );
        self.cleanup_attempts();
        result.map(|mut result| {
            result.changes = best.changes;
            result.consent = best.consent;
            result
        })
    }

    pub fn abandon(mut self) {
        self.cleanup_attempts();
    }

    fn remove_attempt(&mut self, path: &Path) {
        if let Some(position) = self.attempts.iter().position(|attempt| attempt == path) {
            remove_file(path);
            self.attempts.remove(position);
        }
    }

    fn remove_last_attempt(&mut self) {
        let Some(path) = self.attempts.last() else {
            return;
        };
        if self.best.as_ref().map(|best| &best.path) == Some(path) {
            return;
        }
        remove_file(path);
        self.attempts.pop();
    }

    fn cancel_cleanup(&mut self) {
        self.cancelled = true;
        self.best = None;
        self.cleanup_attempts();
    }

    fn cleanup_attempts(&mut self) {
        for path in self.attempts.drain(..) {
            remove_file(&path);
        }
    }
}

fn remove_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("warn: temp cleanup failed for {}: {error}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::processor::SidecarPaths;
    use crate::engine::ProgressSender;
    use crate::settings::QualityFloors;
    use std::path::PathBuf;
    use tokio_util::sync::CancellationToken;

    fn sidecars() -> SidecarPaths {
        SidecarPaths {
            vips: None,
            ffmpeg: None,
            ffprobe: None,
            gs: None,
            qpdf: None,
            gs_resources: None,
        }
    }

    fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        input: PathBuf,
        output: PathBuf,
        temp: PathBuf,
        floors: QualityFloors,
        sidecars: SidecarPaths,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let input = write_file(dir.path(), "in.bin", b"input bytes that are long enough");
            let output = dir.path().join("out.bin");
            let temp = dir.path().join("tmp");
            std::fs::create_dir_all(&temp).unwrap();
            Fixture {
                _dir: dir,
                input,
                output,
                temp,
                floors: QualityFloors::default(),
                sidecars: sidecars(),
            }
        }

        fn ctx(&self, cancel: CancellationToken, progress: ProgressSender) -> JobContext<'_> {
            JobContext::new(
                &self.input,
                &self.output,
                100,
                &self.floors,
                cancel,
                progress,
                &self.sidecars,
                &self.temp,
            )
        }
    }

    #[test]
    fn ladder_keeps_the_smallest_attempt() {
        let fixture = Fixture::new();
        let ctx = fixture.ctx(CancellationToken::new(), ProgressSender::test_stub());
        let first = write_file(&ctx.temp_dir, "attempt-0.bin", &[b'a'; 500]);
        let tie = write_file(&ctx.temp_dir, "attempt-1.bin", &[b'b'; 500]);
        let smallest = write_file(&ctx.temp_dir, "attempt-2.bin", &[b'c'; 400]);
        let mut ladder = Ladder::new();

        for (path, size, strategy) in [
            (first, 500, "first"),
            (tie, 500, "tie"),
            (smallest, 400, "smallest"),
        ] {
            assert!(matches!(
                ladder.record(
                    &ctx,
                    AttemptOutcome::Produced {
                        path,
                        size,
                        quality: 50,
                        strategy: strategy.to_string(),
                        changes: Vec::new(),
                        consent: ConsentSource::Default,
                    }
                ),
                AttemptDecision::Continue
            ));
        }

        let result = ladder.finish(&ctx).unwrap();
        assert_eq!(result.compressed_size, 400, "strictly smallest wins");
        assert_eq!(result.strategy_used, "smallest,best-effort");
        assert!(!result.met_target);
        assert_eq!(std::fs::read(&fixture.output).unwrap(), vec![b'c'; 400]);
        assert_eq!(
            std::fs::read_dir(&ctx.temp_dir).unwrap().count(),
            0,
            "every attempt file must be removed after delivery"
        );
    }

    #[test]
    fn ladder_cleans_attempts_on_cancel() {
        let fixture = Fixture::new();
        let token = CancellationToken::new();
        let ctx = fixture.ctx(token.clone(), ProgressSender::test_stub());
        let first = write_file(&ctx.temp_dir, "attempt-0.bin", b"one");
        let second = write_file(&ctx.temp_dir, "attempt-1.bin", b"two");
        let mut ladder = Ladder::new();
        ladder.track(&first);
        ladder.track(&second);

        token.cancel();
        let decision = ladder.record(
            &ctx,
            AttemptOutcome::Produced {
                path: second.clone(),
                size: 200,
                quality: 40,
                strategy: "cancelled".to_string(),
                changes: Vec::new(),
                consent: ConsentSource::Default,
            },
        );
        assert!(matches!(decision, AttemptDecision::Continue));
        let result = ladder.finish(&ctx);
        assert!(matches!(result, Err(CompressionError::Cancelled)));
        assert!(!first.exists(), "first attempt file must be removed");
        assert!(!second.exists(), "second attempt file must be removed");
        assert!(!fixture.output.exists(), "nothing may be delivered on cancel");
    }

    #[test]
    fn ladder_short_circuits_under_target() {
        let fixture = Fixture::new();
        let ctx = fixture.ctx(CancellationToken::new(), ProgressSender::test_stub());
        let input_size = std::fs::metadata(&ctx.input).unwrap().len();
        let ladder = Ladder::new();

        let result = ladder
            .shortcut(&ctx, input_size)
            .expect("under-target input must take the copy path")
            .expect("under-target input must return the copy result");
        assert!(result.met_target);
        assert_eq!(result.strategy_used, STRATEGY_ALREADY_UNDER_TARGET);
        assert_eq!(result.compressed_size, input_size);
        assert_eq!(
            std::fs::read(&fixture.output).unwrap(),
            std::fs::read(&fixture.input).unwrap()
        );
    }

    #[test]
    fn ladder_shortcut_never_clobbers_an_existing_destination() {
        let fixture = Fixture::new();
        let ctx = fixture.ctx(CancellationToken::new(), ProgressSender::test_stub());
        std::fs::write(&fixture.output, b"precious").unwrap();
        let input_size = std::fs::metadata(&ctx.input).unwrap().len();

        let error = Ladder::new()
            .shortcut(&ctx, input_size)
            .expect_err("an existing destination must fail the copy");
        assert!(
            matches!(error, CompressionError::OutputFileError(_)),
            "got {error:?}"
        );
        assert_eq!(std::fs::read(&fixture.output).unwrap(), b"precious");
        assert!(ctx.input.exists(), "the input must never be moved or removed");
    }

    #[test]
    fn shortcut_propagates_output_file_error() {
        let fixture = Fixture::new();
        let ctx = fixture.ctx(CancellationToken::new(), ProgressSender::test_stub());
        std::fs::write(&fixture.output, b"precious").unwrap();
        let input_size = std::fs::metadata(&ctx.input).unwrap().len();

        assert!(
            ctx.sidecars.vips.is_none(),
            "no engine may be required to observe the copy error"
        );
        let error = Ladder::new()
            .shortcut(&ctx, input_size)
            .expect_err("a copy onto an existing destination must fail");
        assert!(
            matches!(error, CompressionError::OutputFileError(_)),
            "shortcut must surface OutputFileError, got {error:?}"
        );
        assert_eq!(std::fs::read(&fixture.output).unwrap(), b"precious");
        assert!(ctx.input.exists(), "the input must survive a failed copy");
        assert_eq!(
            std::fs::read_dir(ctx.temp_dir).unwrap().count(),
            0,
            "a failed shortcut must not leave attempt files"
        );
    }

    #[test]
    #[cfg(unix)]
    fn shortcut_propagates_cancelled() {
        use std::io::Write;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("in.fifo");
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);

        let output = dir.path().join("out.bin");
        let temp = dir.path().join("tmp");
        let floors = QualityFloors::default();
        let sidecars = sidecars();
        let token = CancellationToken::new();
        let worker_token = token.clone();
        let worker_fifo = fifo.clone();
        let worker_output = output.clone();
        let worker_temp = temp.clone();

        let worker = std::thread::spawn(move || {
            let ctx = JobContext::new(
                &worker_fifo,
                &worker_output,
                2 * 1024 * 1024,
                &floors,
                worker_token,
                ProgressSender::test_stub(),
                &sidecars,
                &worker_temp,
            );
            Ladder::new().shortcut(&ctx, 1024 * 1024)
        });

        let mut writer = std::fs::OpenOptions::new()
            .write(true)
            .open(&fifo)
            .unwrap();
        writer.write_all(&[0u8; 8192]).unwrap();
        token.cancel();
        drop(writer);

        let result = worker.join().unwrap();
        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "cancelled shortcut must surface Cancelled, got {result:?}"
        );
        assert!(!output.exists(), "cancelled shortcut must not deliver");
        assert!(
            !output.with_extension("yard-tmp").exists(),
            "the partial copy must be removed on cancel"
        );
    }

    #[test]
    fn progress_has_one_scale_across_attempts() {
        let fixture = Fixture::new();
        let (progress, seen) = ProgressSender::test_collector();
        let ctx = fixture.ctx(CancellationToken::new(), progress);
        let mut ladder = Ladder::new();
        let attempt_progress = ladder.progress(&ctx);

        attempt_progress.send_progress(50);
        attempt_progress.send_progress(100);
        attempt_progress.send_progress(100);

        let second = ladder.progress(&ctx);
        second.send_progress(10);

        let path = write_file(&ctx.temp_dir, "attempt-0.bin", &[b'x'; 200]);
        ladder.record(
            &ctx,
            AttemptOutcome::Produced {
                path,
                size: 200,
                quality: 40,
                strategy: "over".to_string(),
                changes: Vec::new(),
                consent: ConsentSource::Default,
            },
        );
        ladder.finish(&ctx).unwrap();

        let events = seen.lock().unwrap().clone();
        assert_eq!(
            events,
            vec![45, 90, 100],
            "attempt-local 100 maps to 90 exactly once, never regresses, and delivery emits 100"
        );
    }

    #[test]
    fn no_processor_contains_a_per_attempt_outcome_match() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/processors");
        for file in ["image.rs", "video.rs", "pdf.rs", "document.rs"] {
            let source = std::fs::read_to_string(root.join(file)).unwrap();
            for forbidden in [
                "BestAttempt",
                "deliver_output(",
                "Err(CompressionError::Cancelled) =>",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "{file} still owns cross-attempt behaviour: found `{forbidden}`"
                );
            }
        }
    }
}
