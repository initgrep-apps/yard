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

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::engine::result::{CompressionError, CompressionResult, STRATEGY_ALREADY_UNDER_TARGET};
use crate::engine::{
    BestAttempt, CompressionProcessor, ProgressSender, SidecarName, SidecarPaths, StderrTail,
    deliver_output, stderr_tail,
};
use crate::models::Category;
use crate::settings::QualityFloors;
use crate::sidecar::suppress_console_window;

pub struct VideoProcessor;

impl Default for VideoProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolution ladder per spec §5: 1080p → 720p → 480p. Each rung is the
/// 16:9-equivalent height paired with its cap width; the scale filter
/// implements each rung as a width cap (`min(iw,<width>)`) so a smaller
/// input is never upscaled, and height follows the input aspect ratio.
const RESOLUTION_RUNGS: [(u32, u32); 3] = [(1080, 1920), (720, 1280), (480, 854)];
/// CRF walk ascends toward the quality floor (higher CRF = smaller file).
const BASE_CRF_RUNGS: [u32; 2] = [23, 28];
/// Durations below this cannot yield a sane two-pass bitrate.
const MIN_DURATION_SECS: f64 = 0.5;
const AUDIO_KBPS: u32 = 128;
const CONTAINER_OVERHEAD: f64 = 0.02;
/// Video bitrate below this is not worth encoding; the ABR attempt fails and
/// the ladder moves on to CRF.
const MIN_VIDEO_KBPS: u32 = 50;
const WAIT_POLL_MILLIS: u64 = 30;
/// Cap ffmpeg's stderr is allowed to burst per read chunk.
const PROGRESS_CHUNK_SIZE: usize = 4096;

/// Pass 1 discards its output. Windows has no `/dev/null`; cmd's `NUL`
/// device is the equivalent sink.
#[cfg(windows)]
const PASS1_SINK: &str = "NUL";
#[cfg(not(windows))]
const PASS1_SINK: &str = "/dev/null";

impl VideoProcessor {
    pub fn new() -> Self {
        VideoProcessor
    }

    /// ffprobe prints the duration as decimal seconds on stdout, one line
    /// (`123.456\n`). Empty output, `N/A`, or any other non-number means the
    /// container has no readable duration — the job fails instead of letting
    /// a NaN/zero reach the bitrate math.
    fn parse_duration_output(raw: &str) -> Option<f64> {
        raw.trim()
            .parse::<f64>()
            .ok()
            .filter(|d| d.is_finite() && *d > 0.0)
    }

    /// Spec §5 budget in kbps for ffmpeg's `-b:v`: total bits for the target
    /// less the audio track, then less `CONTAINER_OVERHEAD`. Bit maths uses
    /// f64 because the budget is fractional; the `/ 1000.0` converts bits/s
    /// to kbps. A result below `MIN_VIDEO_KBPS` (or 0) means the ABR attempt
    /// cannot run — the caller rejects it and the CRF ladder continues.
    fn video_bitrate_kbps(target_bytes: u64, duration_secs: f64, audio_kbps: u32) -> u32 {
        if !duration_secs.is_finite()
            || duration_secs < MIN_DURATION_SECS
            || target_bytes == 0
        {
            return 0;
        }
        let total_kbps = target_bytes as f64 * 8.0 * 0.95 / duration_secs / 1000.0;
        let budget = (total_kbps - audio_kbps as f64) * (1.0 - CONTAINER_OVERHEAD);
        if !budget.is_finite() || budget <= 0.0 {
            0
        } else {
            budget.floor() as u32
        }
    }

    /// `HH:MM:SS.ms` (also accepts `MM:SS` / bare seconds) to seconds.
    /// Negative timestamps (exotic containers, `time=-00:00:05`) are
    /// rejected — any negative component means the token is malformed, and
    /// a negative duration share would be nonsense.
    fn parse_time_hms(token: &str) -> Option<f64> {
        let parts: Vec<f64> = token
            .split(':')
            .map(|p| p.trim().parse::<f64>().ok())
            .collect::<Option<Vec<_>>>()?;
        if parts.iter().any(|p| p.is_sign_negative()) {
            return None;
        }
        let secs = match parts.len() {
            3 => Some(parts[0] * 3600.0 + parts[1] * 60.0 + parts[2]),
            2 => Some(parts[0] * 60.0 + parts[1]),
            1 => Some(parts[0]),
            _ => None,
        }?;
        if !secs.is_finite() {
            return None;
        }
        Some(secs)
    }

    /// Extracts the encode position from one ffmpeg stderr line. Accepts both
    /// `time=00:01:23.45` and `out_time_ms=12345678` — the latter is
    /// MICROSECONDS despite the `_ms` name (modern ffmpeg mislabels it).
    /// Garbage, partial, and unrelated lines yield `None`.
    fn progress_line_seconds(line: &str) -> Option<f64> {
        let mut best: Option<f64> = None;
        if let Some(pos) = line.find("out_time_ms=") {
            let rest = &line[pos + "out_time_ms=".len()..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(micros) = digits.parse::<u64>() {
                best = Some(micros as f64 / 1_000_000.0);
            }
        }
        if let Some(pos) = line.find("time=") {
            let rest = &line[pos + "time=".len()..];
            let token = rest.split_ascii_whitespace().next().unwrap_or("");
            if let Some(t) = Self::parse_time_hms(token) {
                best = Some(best.map_or(t, |b| b.max(t)));
            }
        }
        best
    }

    /// Percent of `duration_secs` reached at `secs`, clamped to 0..=100 —
    /// a `time=01:00:01` line on a 01:00:00 video must never emit 100.001%.
    fn seconds_to_percent(secs: f64, duration_secs: f64) -> u32 {
        if duration_secs <= 0.0 || !duration_secs.is_finite() {
            return 0;
        }
        ((secs / duration_secs) * 100.0).clamp(0.0, 100.0) as u32
    }

    /// Percent of `duration_secs` reached, from the maximum timestamp in the
    /// whole stderr. Test-side parser: the live reader thread consumes the
    /// same pieces per line (`progress_line_seconds` + `seconds_to_percent`)
    /// instead of batch-parsing after the fact. Splits on both terminators
    /// so CR-only stats streams behave identically.
    #[cfg(test)]
    fn parse_progress_percent(stderr: &str, duration_secs: f64) -> u32 {
        let max = stderr
            .split(|c| c == '\n' || c == '\r')
            .filter_map(Self::progress_line_seconds)
            .fold(0.0f64, f64::max);
        Self::seconds_to_percent(max, duration_secs)
    }

    /// Resolution rungs allowed by the settings floor (`"480p"` → 480).
    /// Unparseable floors do not restrict the ladder.
    fn resolution_rungs(min_resolution: &str) -> Vec<(u32, u32)> {
        let digits: String = min_resolution.chars().filter(|c| c.is_ascii_digit()).collect();
        let min = digits.parse::<u32>().unwrap_or(0);
        RESOLUTION_RUNGS
            .iter()
            .copied()
            .filter(|(height, _)| *height >= min)
            .collect()
    }

    /// CRF rungs ascending toward the floor: the floor is the WORST allowed
    /// quality (highest CRF), so rungs above it are filtered out and the
    /// floor itself always terminates the walk.
    fn crf_rungs(min_crf: u32) -> Vec<u32> {
        let mut rungs: Vec<u32> = BASE_CRF_RUNGS
            .iter()
            .copied()
            .filter(|c| *c <= min_crf)
            .collect();
        if rungs.last().copied() != Some(min_crf) {
            rungs.push(min_crf);
        }
        rungs
    }

    fn scale_args(scale_width: Option<u32>) -> Vec<String> {
        // `min(iw,…)` means the ladder only ever DOWNSCALES — an input
        // narrower than a rung's cap passes through untouched.
        match scale_width {
            Some(w) => vec!["-vf".to_string(), format!("scale=min(iw\\,{w}):-2")],
            None => Vec::new(),
        }
    }

    fn stream_map_args() -> Vec<&'static str> {
        vec![
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-map",
            "0:s?",
            "-map_chapters",
            "0",
            "-map_metadata",
            "0",
        ]
    }

    /// Output-side flags shared by the ABR and CRF encodes: every audio and
    /// subtitle track is mapped, audio is re-encoded once at
    /// `AUDIO_KBPS`, subtitles become `mov_text` (MP4 has no default
    /// subtitle encoder), colour is forced to 8-bit 4:2:0 for playback
    /// compatibility, and `+faststart` puts `moov` before `mdat`.
    fn output_codec_args() -> Vec<String> {
        let mut args: Vec<String> = Self::stream_map_args()
            .into_iter()
            .map(String::from)
            .collect();
        args.extend(["-c:a", "aac", "-b:a"].into_iter().map(String::from));
        args.push(format!("{AUDIO_KBPS}k"));
        args.extend(
            [
                "-c:s",
                "mov_text",
                "-pix_fmt",
                "yuv420p",
                "-preset",
                "medium",
                "-movflags",
                "+faststart",
            ]
            .into_iter()
            .map(String::from),
        );
        args
    }

    /// Pass 1 (analysis): `-y -i input -c:v libx264 -b:v {br}k [-vf …]
    /// -pix_fmt yuv420p -preset medium -pass 1 -passlogfile <prefix> -an
    /// -f mp4 <sink>`. The sink is last and only pass 1 is audio-less.
    fn pass1_args(
        input: &Path,
        bitrate_k: u32,
        scale_width: Option<u32>,
        passlog_prefix: &Path,
    ) -> Vec<String> {
        let mut args = vec![
            "-y".to_string(),
            "-i".to_string(),
            input.display().to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-b:v".to_string(),
            format!("{bitrate_k}k"),
        ];
        args.extend(Self::scale_args(scale_width));
        args.extend(
            ["-pix_fmt", "yuv420p", "-preset", "medium"]
                .into_iter()
                .map(String::from),
        );
        args.extend(["-map", "0:v:0"].into_iter().map(String::from));
        args.extend([
            "-pass".to_string(),
            "1".to_string(),
            "-passlogfile".to_string(),
            passlog_prefix.display().to_string(),
            "-an".to_string(),
            "-f".to_string(),
            "mp4".to_string(),
            PASS1_SINK.to_string(),
        ]);
        args
    }

    /// Pass 2 (encode): `-i input -c:v libx264 -b:v {br}k [-vf …]
    /// -pass 2 -passlogfile <same prefix> <stream maps and output flags>
    /// output`.
    fn pass2_args(
        input: &Path,
        output: &Path,
        bitrate_k: u32,
        scale_width: Option<u32>,
        passlog_prefix: &Path,
    ) -> Vec<String> {
        let mut args = vec![
            "-i".to_string(),
            input.display().to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-b:v".to_string(),
            format!("{bitrate_k}k"),
        ];
        args.extend(Self::scale_args(scale_width));
        args.extend([
            "-pass".to_string(),
            "2".to_string(),
            "-passlogfile".to_string(),
            passlog_prefix.display().to_string(),
        ]);
        args.extend(Self::output_codec_args());
        args.push(output.display().to_string());
        args
    }

    /// CRF fallback (single pass): `-y -i input -c:v libx264 -crf {crf}
    /// [-vf …] <stream maps and output flags> output`.
    fn crf_args(
        input: &Path,
        output: &Path,
        crf: u32,
        scale_width: Option<u32>,
    ) -> Vec<String> {
        let mut args = vec![
            "-y".to_string(),
            "-i".to_string(),
            input.display().to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-crf".to_string(),
            crf.to_string(),
        ];
        args.extend(Self::scale_args(scale_width));
        args.extend(Self::output_codec_args());
        args.push(output.display().to_string());
        args
    }

    /// An ffmpeg exit 0 is not success on its own — the output must be
    /// non-empty and carry the MP4 `ftyp` box header.
    fn measure_output(path: &Path) -> Result<u64, CompressionError> {
        let mut file = std::fs::File::open(path).map_err(|e| {
            CompressionError::OutputFileError(format!("attempt output ({}): {e}", path.display()))
        })?;
        let mut head = [0u8; 8];
        let mut filled = 0;
        while filled < head.len() {
            let n = file
                .read(&mut head[filled..])
                .map_err(|e| {
                    CompressionError::OutputFileError(format!(
                        "attempt output ({}): {e}",
                        path.display()
                    ))
                })?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled < head.len() || &head[4..8] != b"ftyp" {
            return Err(CompressionError::SidecarFailed(
                "File may be corrupt (sidecar output is not a readable MP4)".to_string(),
            ));
        }
        let size = file
            .metadata()
            .map(|m| m.len())
            .map_err(|e| {
                CompressionError::OutputFileError(format!(
                    "attempt output ({}): {e}",
                    path.display()
                ))
            })?;
        if size == 0 {
            return Err(CompressionError::SidecarFailed(
                "File may be corrupt (sidecar produced an empty output)".to_string(),
            ));
        }
        Ok(size)
    }

    /// Content-equivalence gate: a smaller file that lost a track or a chunk
    /// of duration is not a successful compression. Dropping a stream needs
    /// consent (Task 10); until then the input counts are the floor.
    fn validate_output(
        input: &StreamInfo,
        output: &StreamInfo,
        output_path: &Path,
    ) -> Result<(), CompressionError> {
        let tolerance = input.duration_secs * 0.01;
        if (output.duration_secs - input.duration_secs).abs() > tolerance {
            return Err(CompressionError::SidecarFailed(format!(
                "Output validation failed ({}): duration {:.2}s is not within 1% of the input {:.2}s",
                output_path.display(),
                output.duration_secs,
                input.duration_secs
            )));
        }
        if output.video != 1 {
            return Err(CompressionError::SidecarFailed(format!(
                "Output validation failed ({}): expected one video stream, found {}",
                output_path.display(),
                output.video
            )));
        }
        if output.audio < input.audio {
            return Err(CompressionError::SidecarFailed(format!(
                "Output validation failed ({}): audio tracks {} < input {}",
                output_path.display(),
                output.audio,
                input.audio
            )));
        }
        if output.subtitle < input.subtitle {
            return Err(CompressionError::SidecarFailed(format!(
                "Output validation failed ({}): subtitle tracks {} < input {}",
                output_path.display(),
                output.subtitle,
                input.subtitle
            )));
        }
        Ok(())
    }

    fn remove_all(paths: &[PathBuf]) {
        for path in paths {
            if let Err(e) = std::fs::remove_file(path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("warn: temp cleanup failed for {}: {e}", path.display());
                }
            }
        }
    }

    /// Removes every temp-dir file sharing the passlogfile prefix (ffmpeg
    /// writes `<prefix>-0.log`, `<prefix>-0.log.mbtree`, …). Orphan passlogs
    /// after cancel/error are a regression.
    fn remove_passlogs(temp_dir: &Path, prefix: &Path) {
        let Some(name) = prefix.file_name() else {
            return;
        };
        let name = name.to_string_lossy().into_owned();
        let entries = match std::fs::read_dir(temp_dir) {
            Ok(entries) => entries,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("warn: passlog cleanup read_dir({}) failed: {e}", temp_dir.display());
                }
                return;
            }
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&name) {
                if let Err(e) = std::fs::remove_file(entry.path()) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        eprintln!(
                            "warn: passlog cleanup failed for {}: {e}",
                            entry.path().display()
                        );
                    }
                }
            }
        }
    }

    /// Polls a dispatched child to completion. Cancel SIGKILLs the child and
    /// surfaces as `Cancelled`; a `try_wait` failure kills, reaps, and
    /// surfaces as a sidecar failure. Shared poll loop for ffprobe and the
    /// progress-reader ffmpeg (which need pipes run_sidecar cannot provide).
    fn poll_child_exit(
        child: &mut std::process::Child,
        cancel_token: &CancellationToken,
        label: &str,
    ) -> Result<std::process::ExitStatus, CompressionError> {
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {
                    if cancel_token.is_cancelled() {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(CompressionError::Cancelled);
                    }
                    std::thread::sleep(Duration::from_millis(WAIT_POLL_MILLIS));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CompressionError::SidecarFailed(format!(
                        "{label} wait failed: {e}"
                    )));
                }
            }
        }
    }

    /// Runs ffprobe to completion and returns its stdout. Any non-zero exit
    /// maps to "File may be corrupt" (spec §6): a container ffprobe cannot
    /// read will not survive encoding either.
    fn run_ffprobe(
        ffprobe_path: &str,
        input: &Path,
        cancel_token: &CancellationToken,
    ) -> Result<String, CompressionError> {
        let args = vec![
            "-v".to_string(),
            "error".to_string(),
            "-show_entries".to_string(),
            "format=duration:stream=codec_type".to_string(),
            "-of".to_string(),
            "default=noprint_wrappers=1:nokey=1".to_string(),
            input.display().to_string(),
        ];
        let mut command = std::process::Command::new(ffprobe_path);
        command
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        suppress_console_window(&mut command);
        let mut child = command.spawn().map_err(|e| {
            CompressionError::SidecarFailed(format!("Failed to start ffprobe: {e}"))
        })?;

        let mut stdout_pipe = child.stdout.take();
        let stdout_reader = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(pipe) = stdout_pipe.as_mut() {
                if let Err(e) = pipe.read_to_end(&mut buffer) {
                    eprintln!("warn: ffprobe stdout drain failed: {e}");
                }
            }
            buffer
        });
        let mut stderr_pipe = child.stderr.take();
        let stderr_reader = std::thread::spawn(move || {
            let mut tail = StderrTail::new();
            if let Some(pipe) = stderr_pipe.as_mut() {
                let mut buf = [0u8; PROGRESS_CHUNK_SIZE];
                loop {
                    match pipe.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => tail.push(&buf[..n]),
                        Err(e) => {
                            if e.kind() == std::io::ErrorKind::Interrupted {
                                continue;
                            }
                            eprintln!("warn: ffprobe stderr drain failed, diagnostics partial: {e}");
                            break;
                        }
                    }
                }
            }
            tail
        });

        let status = Self::poll_child_exit(&mut child, cancel_token, "ffprobe")?;
        if cancel_token.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }
        let stdout = stdout_reader.join().unwrap_or_default();
        let stderr_tail_struct = stderr_reader.join().unwrap_or_default();

        if !status.success() {
            return Err(CompressionError::SidecarFailed(format!(
                "File may be corrupt (ffprobe exited with code {:?}): {}",
                status.code(),
                stderr_tail(&stderr_tail_struct)
            )));
        }
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    }

    fn parse_stream_info(raw: &str) -> Option<StreamInfo> {
        let mut duration_secs = None;
        let mut video = 0;
        let mut audio = 0;
        let mut subtitle = 0;
        for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
            match line {
                "video" => video += 1,
                "audio" => audio += 1,
                "subtitle" => subtitle += 1,
                _ => {
                    if duration_secs.is_none() {
                        duration_secs = Self::parse_duration_output(line);
                    }
                }
            }
        }
        duration_secs.map(|duration_secs| StreamInfo {
            duration_secs,
            video,
            audio,
            subtitle,
        })
    }

    fn probe_streams_with_cancel(
        path: &Path,
        ffprobe_path: &str,
        cancel_token: &CancellationToken,
    ) -> Result<StreamInfo, CompressionError> {
        let raw = Self::run_ffprobe(ffprobe_path, path, cancel_token)?;
        Self::parse_stream_info(&raw).ok_or_else(|| {
            CompressionError::SidecarFailed(
                "File may be corrupt (ffprobe reported no readable duration)".to_string(),
            )
        })
    }

    fn probe_streams(path: &Path, ffprobe_path: &str) -> Result<StreamInfo, CompressionError> {
        Self::probe_streams_with_cancel(path, ffprobe_path, &CancellationToken::new())
    }

    /// Drains an ffmpeg stderr pipe into progress events, retaining a
    /// bounded tail in the shared `StderrTail`. Live percent tracking happens
    /// entirely in the caller's `line_sink`; this function does not parse
    /// progress.
    ///
    /// Lines end at `\n` OR `\r`: ffmpeg marks every `-stats` frame update
    /// with a carriage return and emits a single `\n` only for the final
    /// post-exit summary, so draining on `\n` alone would hold every frame
    /// update in `pending` until the process exits — no live progress for
    /// the whole encode, and an unbounded buffer on long videos. Raw bytes
    /// are lossy-converted per complete line, so a non-UTF-8 filename
    /// embedded in stderr neither freezes progress nor truncates
    /// diagnostics, and a progress token straddling read chunks is held
    /// over until its terminator arrives.
    ///
    /// `line_sink` receives each complete line (terminator included) the
    /// moment it is peeled off — the reader thread uses it to parse and
    /// emit progress; tests use it to observe exactly when lines drain.
    #[doc(hidden)]
    pub(crate) fn drain_stderr_for_progress<R: std::io::Read>(
        pipe: &mut R,
        line_sink: &mut dyn FnMut(&[u8]),
        tail: &mut StderrTail,
    ) {
        let mut chunk = [0u8; PROGRESS_CHUNK_SIZE];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    tail.push(&chunk[..n]);
                    pending.extend_from_slice(&chunk[..n]);
                    // Drain up to the LAST terminator so lines arriving as
                    // one burst are each parsed separately.
                    if let Some(pos) = pending.iter().rposition(|b| *b == b'\n' || *b == b'\r') {
                        let complete: Vec<u8> = pending.drain(..=pos).collect();
                        line_sink(&complete);
                    }
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    eprintln!("warn: ffmpeg stderr drain failed, progress partial: {e}");
                    // Keep the tail already drained; diagnostics stay partial.
                    break;
                }
            }
        }
        // EOF: whatever has no trailing terminator is still real output.
        if !pending.is_empty() {
            line_sink(&pending);
        }
    }

    /// Runs one ffmpeg invocation with live progress: stderr is streamed line
    /// by line (CR- or LF-terminated — ffmpeg `-stats` uses `\r` for frame
    /// updates), each line parsed for `time=` / `out_time_ms=`, mapped into
    /// this pass's slice of the overall percent range and sent as it
    /// arrives. Cancel SIGKILLs the child and surfaces as `Cancelled`.
    #[allow(clippy::too_many_arguments)]
    fn run_ffmpeg_with_progress(
        ffmpeg_path: &str,
        args: &[String],
        cancel_token: &CancellationToken,
        progress: &ProgressSender,
        range_base: u32,
        range_span: u32,
        duration_secs: f64,
    ) -> Result<(), CompressionError> {
        let mut command = std::process::Command::new(ffmpeg_path);
        command
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        suppress_console_window(&mut command);
        let mut child = command.spawn().map_err(|e| {
            CompressionError::SidecarFailed(format!("Failed to start ffmpeg: {e}"))
        })?;

        let mut stderr_pipe = child.stderr.take();
        let progress_for_thread = progress.clone();
        let reader = std::thread::spawn(move || {
            let mut max_percent: u32 = 0;
            let mut tail = StderrTail::new();
            if let Some(pipe) = stderr_pipe.as_mut() {
                let parse_and_record = |bytes: &[u8], max_percent: &mut u32| {
                    let text = String::from_utf8_lossy(bytes);
                    // Split on both terminators: one drained chunk can carry
                    // several \r-separated stats lines, and str::lines()
                    // only walks \n.
                    for line in text.split(|c| c == '\n' || c == '\r') {
                        if let Some(secs) = Self::progress_line_seconds(line.trim()) {
                            let pct = Self::seconds_to_percent(secs, duration_secs);
                            *max_percent = (*max_percent).max(pct);
                            if range_span > 0 {
                                progress_for_thread
                                    .send_progress(range_base + (pct * range_span) / 100);
                            }
                        }
                    }
                };
                let mut drain_sink = |bytes: &[u8]| parse_and_record(bytes, &mut max_percent);
                Self::drain_stderr_for_progress(pipe, &mut drain_sink, &mut tail);
            }
            (max_percent, tail)
        });
        let status = Self::poll_child_exit(&mut child, cancel_token, "ffmpeg")?;
        if cancel_token.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }
        let (max_percent, tail) = match reader.join() {
            Ok(pair) => pair,
            Err(_) => {
                eprintln!("warn: ffmpeg stderr drain thread panicked");
                (0, StderrTail::new())
            }
        };

        if !status.success() {
            return Err(CompressionError::SidecarFailed(format!(
                "ffmpeg exited with code {:?}: {}",
                status.code(),
                stderr_tail(&tail)
            )));
        }
        if range_span > 0 {
            let scaled = (max_percent * range_span) / 100;
            progress.send_progress(range_base + scaled);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct StreamInfo {
    duration_secs: f64,
    video: u32,
    audio: u32,
    subtitle: u32,
}

enum VideoAttempt {
    TwoPass {
        scale_width: Option<u32>,
        /// Passlogfile prefix — TwoPass MUST carry one (no passlog means a
        /// missing pass-2 input, not a recoverable failure). Held by the
        /// variant so there is no `Option` param that invites a panic.
        passlog_prefix: PathBuf,
    },
    Crf {
        crf: u32,
        scale_width: Option<u32>,
    },
}

impl VideoAttempt {
    fn strategy(&self, bitrate_k: u32) -> String {
        match self {
            VideoAttempt::TwoPass { scale_width, .. } => match scale_width {
                Some(w) => format!("two-pass h264 b={bitrate_k}k scale={w}"),
                None => format!("two-pass h264 b={bitrate_k}k"),
            },
            VideoAttempt::Crf { crf, scale_width } => match scale_width {
                Some(w) => format!("crf={crf} scale={w}"),
                None => format!("crf={crf}"),
            },
        }
    }

    fn quality(&self, bitrate_k: u32) -> u32 {
        match self {
            VideoAttempt::TwoPass { .. } => bitrate_k,
            VideoAttempt::Crf { crf, .. } => *crf,
        }
    }
}

impl CompressionProcessor for VideoProcessor {
    fn category(&self) -> Category {
        Category::Video
    }

    fn supported_extensions(&self) -> &[&str] {
        &["mp4", "mov"]
    }

    fn compress_to_target(
        &self,
        input: &Path,
        output: &Path,
        target_bytes: u64,
        quality_floor: &QualityFloors,
        cancel_token: CancellationToken,
        progress: ProgressSender,
        sidecar_paths: &SidecarPaths,
        temp_dir: &Path,
    ) -> Result<CompressionResult, CompressionError> {
        let input_size = std::fs::metadata(input)
            .map(|m| m.len())
            .map_err(|e| CompressionError::InputFileError(e.to_string()))?;

        // Already under target — copy as-is, no sidecar spawn (spec §6).
        // Checked BEFORE requiring ffmpeg/ffprobe so a video under target
        // never fails on missing sidecars.
        if input_size <= target_bytes {
            return deliver_output(
                input,
                output,
                input_size,
                STRATEGY_ALREADY_UNDER_TARGET.to_string(),
                100,
                true,
                &progress,
                &cancel_token,
            );
        }

        let ffmpeg_path = sidecar_paths.require(SidecarName::Ffmpeg)?;
        let ffprobe_path = sidecar_paths.require(SidecarName::Ffprobe)?;

        let input_streams = Self::probe_streams_with_cancel(input, ffprobe_path, &cancel_token)?;
        let duration_secs = input_streams.duration_secs;
        if duration_secs < MIN_DURATION_SECS {
            return Err(CompressionError::SidecarFailed(
                "Cannot calculate target bitrate (duration too short)".to_string(),
            ));
        }
        if input_streams.video == 0 {
            return Err(CompressionError::SidecarFailed(
                "File may be corrupt (no video stream)".to_string(),
            ));
        }
        let bitrate_k = Self::video_bitrate_kbps(target_bytes, duration_secs, AUDIO_KBPS);

        // The .mov → .mp4 output naming is applied by the queue (spec §3
        // format table) BEFORE collision resolution, so the path already
        // ends .mp4 here — overwriting an existing file would never be
        // caught as a rename collision.
        let final_output = output.to_path_buf();

        let res_rungs = Self::resolution_rungs(&quality_floor.video.min_resolution);
        let crf_values = Self::crf_rungs(quality_floor.video.min_crf);

        let mut smallest: Option<BestAttempt> = None;
        let mut attempt_outputs: Vec<PathBuf> = Vec::new();
        let mut passlog_prefixes: Vec<PathBuf> = Vec::new();
        let mut last_error: Option<CompressionError> = None;

        // One ABR encode at the highest allowed resolution; CRF then takes
        // over at the lower resolutions, where the size actually changes.
        let mut attempts: Vec<VideoAttempt> = Vec::new();
        if bitrate_k >= MIN_VIDEO_KBPS {
            attempts.push(VideoAttempt::TwoPass {
                scale_width: res_rungs.first().map(|(_, width)| *width),
                passlog_prefix: temp_dir.join("passlog-attempt-0"),
            });
        } else {
            eprintln!(
                "warn: video ABR skipped, {bitrate_k} kbps is below the {MIN_VIDEO_KBPS} kbps floor"
            );
            last_error = Some(CompressionError::SidecarFailed(format!(
                "Cannot calculate target bitrate (below the {MIN_VIDEO_KBPS} kbps floor)"
            )));
        }
        let mut crf_widths: Vec<Option<u32>> = res_rungs
            .iter()
            .skip(1)
            .map(|(_, width)| Some(*width))
            .collect();
        if crf_widths.is_empty() {
            crf_widths.push(res_rungs.first().map(|(_, width)| *width));
        }
        for width in crf_widths {
            for crf in &crf_values {
                attempts.push(VideoAttempt::Crf {
                    crf: *crf,
                    scale_width: width,
                });
            }
        }

        let total_attempts = attempts.len();
        let span = 95 / total_attempts.max(1);

        let cancel_cleanup = |outputs: &[PathBuf], prefixes: &[PathBuf]| {
            Self::remove_all(outputs);
            for prefix in prefixes {
                Self::remove_passlogs(temp_dir, prefix);
            }
        };

        let run_attempt = |attempt: &VideoAttempt,
                           temp_output: &Path,
                           base: u32|
         -> Result<u64, CompressionError> {
            match attempt {
                VideoAttempt::TwoPass {
                    scale_width,
                    passlog_prefix,
                } => {
                    let half = (span / 2) as u32;
                    let pass1 = Self::pass1_args(input, bitrate_k, *scale_width, passlog_prefix);
                    Self::run_ffmpeg_with_progress(
                        ffmpeg_path,
                        &pass1,
                        &cancel_token,
                        &progress,
                        base,
                        half,
                        duration_secs,
                    )?;
                    let pass2 =
                        Self::pass2_args(input, temp_output, bitrate_k, *scale_width, passlog_prefix);
                    Self::run_ffmpeg_with_progress(
                        ffmpeg_path,
                        &pass2,
                        &cancel_token,
                        &progress,
                        base + half,
                        span as u32 - half,
                        duration_secs,
                    )?;
                }
                VideoAttempt::Crf { crf, scale_width } => {
                    let args = Self::crf_args(input, temp_output, *crf, *scale_width);
                    Self::run_ffmpeg_with_progress(
                        ffmpeg_path,
                        &args,
                        &cancel_token,
                        &progress,
                        base,
                        span as u32,
                        duration_secs,
                    )?;
                }
            }
            let size = Self::measure_output(temp_output)?;
            let output_streams = Self::probe_streams(temp_output, ffprobe_path)?;
            Self::validate_output(&input_streams, &output_streams, temp_output)?;
            Ok(size)
        };

        for (index, attempt) in attempts.iter().enumerate() {
            if cancel_token.is_cancelled() {
                cancel_cleanup(&attempt_outputs, &passlog_prefixes);
                return Err(CompressionError::Cancelled);
            }

            let temp_output = temp_dir.join(format!("attempt-{index}.mp4"));
            let passlog_prefix = match attempt {
                VideoAttempt::TwoPass { passlog_prefix, .. } => Some(passlog_prefix.clone()),
                VideoAttempt::Crf { .. } => None,
            };
            attempt_outputs.push(temp_output.clone());
            if let Some(prefix) = &passlog_prefix {
                passlog_prefixes.push(prefix.clone());
            }
            let base = (index * span) as u32;
            progress.send_progress(base);

            let outcome = run_attempt(attempt, &temp_output, base);

            match outcome {
                Ok(size) => {
                    if size <= target_bytes {
                        return deliver_output(
                            &temp_output,
                            &final_output,
                            size,
                            attempt.strategy(bitrate_k),
                            attempt.quality(bitrate_k),
                            true,
                            &progress,
                            &cancel_token,
                        );
                    }
                    BestAttempt::keep_smaller_in(
                        &mut smallest,
                        BestAttempt {
                            size,
                            path: temp_output.clone(),
                            strategy: attempt.strategy(bitrate_k),
                            quality: attempt.quality(bitrate_k),
                        },
                    );
                }
                Err(CompressionError::Cancelled) => {
                    cancel_cleanup(&attempt_outputs, &passlog_prefixes);
                    return Err(CompressionError::Cancelled);
                }
                Err(e) => {
                    eprintln!("warn: video attempt failed, trying next: {e}");
                    last_error = Some(e);
                    Self::remove_all(std::slice::from_ref(&temp_output));
                }
            }
            if let Some(prefix) = &passlog_prefix {
                Self::remove_passlogs(temp_dir, prefix);
            }
        }

        if let Some(best) = smallest {
            return deliver_output(
                &best.path,
                &final_output,
                best.size,
                format!("{},best-effort", best.strategy),
                best.quality,
                false,
                &progress,
                &cancel_token,
            );
        }

        Err(last_error.unwrap_or_else(|| {
            CompressionError::SidecarFailed("Could not compress video".to_string())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Instant;

    /// BufferedReader over explicit byte chunks with per-read delays: exact
    /// chunk sizes reach `drain_stderr_for_progress` even when the data is
    /// already sitting in the pipe buffer, so boundary/straddle cases are
    /// deterministic.
    #[cfg(test)]
    struct ChunkedReader {
        chunks: Vec<Vec<u8>>,
        index: usize,
    }

    #[cfg(test)]
    impl ChunkedReader {
        fn new(chunks: &[&[u8]]) -> Self {
            Self {
                chunks: chunks.iter().map(|c| c.to_vec()).collect(),
                index: 0,
            }
        }
    }

    #[cfg(test)]
    impl std::io::Read for ChunkedReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.index >= self.chunks.len() {
                return Ok(0);
            }
            let chunk = &self.chunks[self.index];
            let n = chunk.len().min(buf.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            self.index += 1;
            Ok(n)
        }
    }

    /// Wraps a reader so one specific `read` call fails with `Interrupted`,
    /// simulating the transient errors real pipes produce.
    #[cfg(test)]
    struct InterruptingReader<R> {
        inner: R,
        fail_on: usize,
        calls: usize,
    }

    #[cfg(test)]
    impl<R: std::io::Read> std::io::Read for InterruptingReader<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if self.calls == self.fail_on {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "simulated interrupt",
                ));
            }
            self.inner.read(buf)
        }
    }

    /// Collects drained lines as owned strings for sequence assertions.
    #[cfg(test)]
    fn drain_lines<R: std::io::Read>(
        pipe: &mut R,
    ) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        {
            let mut sink = |bytes: &[u8]| {
                lines.push(String::from_utf8_lossy(bytes).into_owned());
            };
            let mut tail = StderrTail::new();
            VideoProcessor::drain_stderr_for_progress(pipe, &mut sink, &mut tail);
        }
        lines
    }

    #[cfg(unix)]
    fn make_executable_script(dir: &Path, name: &str, script: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().to_string()
    }

    #[cfg(unix)]
    fn test_sidecar_paths(ffmpeg: Option<String>, ffprobe: Option<String>) -> SidecarPaths {
        SidecarPaths {
            vips: None,
            ffmpeg,
            ffprobe,
            gs: None,
            qpdf: None,
            gs_resources: None,
        }
    }

    #[cfg(unix)]
    fn oversized_input(dir: &Path, name: &str) -> PathBuf {
        let input = dir.join(name);
        fs::write(&input, vec![0u8; 300 * 1024]).unwrap();
        input
    }

    #[cfg(unix)]
    fn temp_dir_under(dir: &Path) -> PathBuf {
        let tmp = dir.join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    /// Writes a valid MP4 header + `size` zero bytes. Pass 1 must NOT create
    /// the output (its sink is /dev/null); it must materialize the passlog.
    #[cfg(unix)]
    fn fake_ffmpeg(dir: &Path, bitrate_size: usize, vf_size: usize, crf_sizes: &str) -> String {
        make_executable_script(
            dir,
            "fake-ffmpeg",
            &format!(
                r#"#!/bin/bash
echo "ffmpeg $*" >> "$(dirname "$0")/ffmpeg.log"
pass=""; pl=""; w=""; crf=""
prev=""
for a in "$@"; do
  case "$prev" in
    -pass) pass="$a" ;;
    -passlogfile) pl="$a" ;;
    -vf) w="${{a#scale=}}"; w="${{w%%:*}}" ;;
    -crf) crf="$a" ;;
  esac
  prev="$a"
done
if [ "$pass" = "1" ]; then
  if [ -n "$pl" ]; then printf 'x' > "${{pl}}-0.log"; printf 'x' > "${{pl}}-0.log.mbtree"; fi
  exit 0
fi
out="${{@: -1}}"
size=0
if [ -n "$crf" ]; then
  case "$crf" in
    {crf_sizes}
    *) size={bitrate_size} ;;
  esac
elif [ -n "$w" ]; then
  size={vf_size}
else
  size={bitrate_size}
fi
printf '\x00\x00\x00\x18ftypmp42' > "$out"
head -c $(( size - 12 )) /dev/zero >> "$out"
"#,
                bitrate_size = bitrate_size,
                vf_size = vf_size,
                crf_sizes = crf_sizes,
            ),
        )
    }

    #[cfg(unix)]
    fn fake_ffmpeg_sleeper(dir: &Path) -> String {
        make_executable_script(
            dir,
            "fake-ffmpeg-sleeper",
            r#"#!/bin/bash
pass=""; pl=""; prev=""
for a in "$@"; do
  case "$prev" in
    -pass) pass="$a" ;;
    -passlogfile) pl="$a" ;;
  esac
  prev="$a"
done
echo "ffmpeg $*" >> "$(dirname "$0")/ffmpeg.log"
if [ "$pass" = "1" ]; then
  if [ -n "$pl" ]; then printf 'x' > "${pl}-0.log"; printf 'x' > "${pl}-0.log.mbtree"; fi
  sleep 60
fi
"#,
        )
    }

    #[cfg(unix)]
    fn fake_ffprobe(dir: &Path, input_lines: &[&str], output_lines: &[&str]) -> String {
        fn printf_lines(lines: &[&str]) -> String {
            lines
                .iter()
                .map(|line| format!("'{line}'"))
                .collect::<Vec<_>>()
                .join(" ")
        }
        make_executable_script(
            dir,
            "fake-ffprobe",
            &format!(
                r#"#!/bin/bash
echo "ffprobe $*" >> "$(dirname "$0")/ffprobe.log"
last="${{@: -1}}"
case "$last" in
  *attempt-*) printf '%s\n' {output} ;;
  *) printf '%s\n' {input} ;;
esac
"#,
                output = printf_lines(output_lines),
                input = printf_lines(input_lines),
            ),
        )
    }

    fn args_contain(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|pair| pair[0] == flag && pair[1] == value)
    }

    #[cfg(unix)]
    fn fake_ffprobe_failing(dir: &Path, message: &str) -> String {
        make_executable_script(
            dir,
            "fake-ffprobe-fail",
            &format!(
                r#"#!/bin/bash
echo "ffprobe $*" >> "$(dirname "$0")/ffprobe.log"
echo "{message}" >&2
exit 1
"#,
                message = message,
            ),
        )
    }

    fn ffmpeg_log(dir: &Path) -> Vec<String> {
        fs::read_to_string(dir.join("ffmpeg.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    // ---------- pure-function tests ----------

    #[test]
    fn ffprobe_parses_duration_decimal_and_missing_entries_fail() {
        assert_eq!(VideoProcessor::parse_duration_output("123.456\n"), Some(123.456));
        assert_eq!(VideoProcessor::parse_duration_output("0.5"), Some(0.5));
        assert_eq!(VideoProcessor::parse_duration_output("  42.0  "), Some(42.0));
        assert_eq!(VideoProcessor::parse_duration_output(""), None);
        assert_eq!(VideoProcessor::parse_duration_output("\n"), None);
        assert_eq!(VideoProcessor::parse_duration_output("N/A"), None);
        assert_eq!(VideoProcessor::parse_duration_output("garbage"), None);
        assert_eq!(VideoProcessor::parse_duration_output("-3.5"), None);
        assert_eq!(VideoProcessor::parse_duration_output("0"), None);
        assert_eq!(VideoProcessor::parse_duration_output("inf"), None);
        assert_eq!(VideoProcessor::parse_duration_output("NaN"), None);
    }

    #[test]
    fn ffprobe_stream_entries_parse_in_real_output_order() {
        // Real ffprobe prints stream entries before the format duration.
        let info = VideoProcessor::parse_stream_info("video\naudio\naudio\nsubtitle\n12.5\n")
            .expect("duration and streams");
        assert_eq!(
            info,
            StreamInfo {
                duration_secs: 12.5,
                video: 1,
                audio: 2,
                subtitle: 1,
            }
        );
        // No duration means no usable probe.
        assert_eq!(VideoProcessor::parse_stream_info("video\naudio\n"), None);
    }

    #[test]
    fn video_bitrate_formula_matches_spec() {
        // 10 MiB over 60 s at 128 kbps audio: 1328.20 kbps total, minus
        // audio = 1200.20, times 0.98 overhead = 1176.19 → 1176.
        let target = 10 * 1024 * 1024;
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(target, 60.0, AUDIO_KBPS),
            1176
        );
        // 2 MiB over 100 s leaves 30 kbps after audio and overhead — below
        // the 50 kbps floor, so the ABR attempt must not run.
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(2 * 1024 * 1024, 100.0, AUDIO_KBPS),
            30
        );
        // Tiny-duration guard: below MIN_DURATION_SECS there is no sane bitrate.
        assert_eq!(VideoProcessor::video_bitrate_kbps(target, 0.0, AUDIO_KBPS), 0);
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(target, 0.4, AUDIO_KBPS),
            0
        );
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(target, -5.0, AUDIO_KBPS),
            0
        );
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(target, f64::NAN, AUDIO_KBPS),
            0
        );
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(target, f64::INFINITY, AUDIO_KBPS),
            0
        );
        // Zero target guard.
        assert_eq!(VideoProcessor::video_bitrate_kbps(0, 60.0, AUDIO_KBPS), 0);
        // Overflow guard: the f64 budget saturates instead of wrapping.
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(u64::MAX, 1.0, AUDIO_KBPS),
            u32::MAX
        );
        // Target too small to pay audio → never -b:v 0k or negative.
        assert_eq!(VideoProcessor::video_bitrate_kbps(1, 3600.0, AUDIO_KBPS), 0);
    }

    #[test]
    fn bitrate_budget_subtracts_audio_and_overhead() {
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(10 * 1024 * 1024, 60.0, AUDIO_KBPS),
            1176
        );
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(10 * 1024 * 1024, 60.0, 0),
            1301
        );
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(5 * 1024 * 1024, 120.0, AUDIO_KBPS),
            199
        );
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(2 * 1024 * 1024, 100.0, 0),
            156
        );
        assert_eq!(
            VideoProcessor::video_bitrate_kbps(2 * 1024 * 1024, 100.0, AUDIO_KBPS),
            30
        );
    }

    #[test]
    fn stream_maps_preserve_all_audio_and_subtitles() {
        assert_eq!(
            VideoProcessor::stream_map_args(),
            vec![
                "-map",
                "0:v:0",
                "-map",
                "0:a?",
                "-map",
                "0:s?",
                "-map_chapters",
                "0",
                "-map_metadata",
                "0",
            ]
        );
        let input = Path::new("/in/video.mp4");
        let prefix = Path::new("/tmp/app/passlog-attempt-0");
        let pass2 = VideoProcessor::pass2_args(
            input,
            Path::new("/tmp/app/attempt-0.mp4"),
            1328,
            None,
            prefix,
        );
        let crf = VideoProcessor::crf_args(input, Path::new("/tmp/app/attempt-1.mp4"), 23, None);
        for args in [&pass2, &crf] {
            for (flag, value) in [
                ("-map", "0:v:0"),
                ("-map", "0:a?"),
                ("-map", "0:s?"),
                ("-map_chapters", "0"),
                ("-map_metadata", "0"),
            ] {
                assert!(args_contain(args, flag, value), "missing {flag} {value}: {args:?}");
            }
        }
    }

    #[test]
    fn final_attempt_forces_8bit_faststart() {
        let input = Path::new("/in/video.mp4");
        let prefix = Path::new("/tmp/app/passlog-attempt-0");
        let pass2 = VideoProcessor::pass2_args(
            input,
            Path::new("/tmp/app/attempt-0.mp4"),
            1328,
            None,
            prefix,
        );
        let crf = VideoProcessor::crf_args(input, Path::new("/tmp/app/attempt-1.mp4"), 23, None);
        for args in [&pass2, &crf] {
            assert!(args_contain(args, "-pix_fmt", "yuv420p"), "got: {args:?}");
            assert!(
                args_contain(args, "-movflags", "+faststart"),
                "got: {args:?}"
            );
            assert!(args_contain(args, "-preset", "medium"), "got: {args:?}");
        }
    }

    #[test]
    fn subtitle_encoder_is_explicit() {
        let input = Path::new("/in/video.mp4");
        let prefix = Path::new("/tmp/app/passlog-attempt-0");
        let pass2 = VideoProcessor::pass2_args(
            input,
            Path::new("/tmp/app/attempt-0.mp4"),
            1328,
            None,
            prefix,
        );
        let crf = VideoProcessor::crf_args(input, Path::new("/tmp/app/attempt-1.mp4"), 23, None);
        for args in [&pass2, &crf] {
            assert!(args_contain(args, "-c:s", "mov_text"), "got: {args:?}");
        }
    }

    #[test]
    fn video_two_pass_commands_constructed_correctly() {
        let input = Path::new("/in/video.mp4");
        let prefix = Path::new("/tmp/app/passlog-attempt-0");
        let pass1 = VideoProcessor::pass1_args(input, 1328, None, prefix);
        assert_eq!(
            pass1,
            vec![
                "-y",
                "-i",
                "/in/video.mp4",
                "-c:v",
                "libx264",
                "-b:v",
                "1328k",
                "-pix_fmt",
                "yuv420p",
                "-preset",
                "medium",
                "-map",
                "0:v:0",
                "-pass",
                "1",
                "-passlogfile",
                "/tmp/app/passlog-attempt-0",
                "-an",
                "-f",
                "mp4",
                PASS1_SINK,
            ]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
        );
        let pass2 = VideoProcessor::pass2_args(
            input,
            Path::new("/tmp/app/attempt-0.mp4"),
            1328,
            None,
            prefix,
        );
        assert_eq!(
            pass2,
            vec![
                "-i",
                "/in/video.mp4",
                "-c:v",
                "libx264",
                "-b:v",
                "1328k",
                "-pass",
                "2",
                "-passlogfile",
                "/tmp/app/passlog-attempt-0",
                "-map",
                "0:v:0",
                "-map",
                "0:a?",
                "-map",
                "0:s?",
                "-map_chapters",
                "0",
                "-map_metadata",
                "0",
                "-c:a",
                "aac",
                "-b:a",
                "128k",
                "-c:s",
                "mov_text",
                "-pix_fmt",
                "yuv420p",
                "-preset",
                "medium",
                "-movflags",
                "+faststart",
                "/tmp/app/attempt-0.mp4",
            ]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
        );
        assert!(pass1.contains(&"-an".to_string()));
        assert!(!pass2.contains(&"-an".to_string()), "got: {pass2:?}");
        // Scale rung lands on BOTH passes, before the -pass flags. The cap
        // filter never upscales: min(iw,<width>).
        let pass1_scaled = VideoProcessor::pass1_args(input, 1328, Some(1280), prefix);
        let vf_pos = pass1_scaled.iter().position(|a| a == "-vf").unwrap();
        assert_eq!(pass1_scaled[vf_pos + 1], "scale=min(iw\\,1280):-2");
        assert!(pass1_scaled.iter().position(|a| a == "-pass").unwrap() > vf_pos);
        let pass2_scaled = VideoProcessor::pass2_args(
            input,
            Path::new("/tmp/app/attempt-0.mp4"),
            1328,
            Some(1280),
            prefix,
        );
        let vf_pos2 = pass2_scaled.iter().position(|a| a == "-vf").unwrap();
        assert_eq!(pass2_scaled[vf_pos2 + 1], "scale=min(iw\\,1280):-2");
        assert!(pass2_scaled.iter().position(|a| a == "-pass").unwrap() > vf_pos2);
    }

    #[test]
    fn video_progress_parses_time_and_out_time_ms() {
        // `time=` HH:MM:SS.ms format.
        assert_eq!(
            VideoProcessor::progress_line_seconds("frame= 30 fps=25 time=00:01:23.45 bitrate=1234.0kbits/s"),
            Some(83.45)
        );
        // `out_time_ms=` is MICROSECONDS despite the name.
        assert_eq!(
            VideoProcessor::progress_line_seconds("frame=1 out_time_ms=12345678"),
            Some(12.345678)
        );
        // Shorter HH:MM:SS precision.
        assert_eq!(
            VideoProcessor::progress_line_seconds("time=00:00:10.00"),
            Some(10.0)
        );
        // MM:SS variant.
        assert_eq!(
            VideoProcessor::progress_line_seconds("frame= 4 time=00:30.00"),
            Some(30.0)
        );
        // Bare seconds.
        assert_eq!(VideoProcessor::progress_line_seconds("time=42"), Some(42.0));
        // Negative tokens are malformed, never progress.
        assert_eq!(VideoProcessor::progress_line_seconds("time=-00:00:05"), None);
        assert_eq!(VideoProcessor::progress_line_seconds("time=-5"), None);
        // Garbage / partial / unrelated lines are ignored.
        assert_eq!(VideoProcessor::progress_line_seconds("Press [q] to stop"), None);
        assert_eq!(VideoProcessor::progress_line_seconds("frame= 12 fps= 5"), None);
        assert_eq!(VideoProcessor::progress_line_seconds(""), None);
        assert_eq!(VideoProcessor::progress_line_seconds("time=garbage"), None);
        assert_eq!(VideoProcessor::progress_line_seconds("out_time_ms=N/A"), None);
        // Multi-line stderr: maximum timestamp wins.
        let stderr = "Press [q] to stop\ntime=00:00:05.00\nframe= 99 time=00:00:12.50\nspeed=1x\n";
        assert_eq!(VideoProcessor::parse_progress_percent(stderr, 100.0), 12);
        // Mixed formats in one stream — both accepted.
        let mixed = "out_time_ms=3000000\ntime=00:00:05.00\n";
        assert_eq!(VideoProcessor::parse_progress_percent(mixed, 100.0), 5);
        // Real ffmpeg `-stats` emits carriage-return-terminated lines; every
        // terminator variant must peel into lines.
        let cr = "frame= 4 time=00:00:02.00\rframe= 9 time=00:00:09.00\r";
        assert_eq!(VideoProcessor::parse_progress_percent(cr, 20.0), 45);
        // Interleaved terminators (a stray \n inside stats bursts still parses).
        let mixed_terms = "time=00:00:01.00\r\ntime=00:00:03.00\ntime=00:00:15.00\r";
        assert_eq!(VideoProcessor::parse_progress_percent(mixed_terms, 20.0), 75);
    }

    #[test]
    fn video_progress_percent_clamped_0_to_100() {
        // time beyond the duration must clamp to exactly 100.
        assert_eq!(
            VideoProcessor::parse_progress_percent("time=01:00:01.00", 60.0),
            100
        );
        assert_eq!(VideoProcessor::parse_progress_percent("time=01:00:00.00", 60.0), 100);
        // Before any progress: 0.
        assert_eq!(VideoProcessor::parse_progress_percent("no progress yet", 60.0), 0);
        assert_eq!(VideoProcessor::parse_progress_percent("time=00:00:00.00", 60.0), 0);
        // Mid-range stays proportional.
        assert_eq!(VideoProcessor::parse_progress_percent("time=00:00:30.00", 60.0), 50);
        // out_time_ms beyond duration also clamps.
        assert_eq!(
            VideoProcessor::parse_progress_percent("out_time_ms=999999999", 60.0),
            100
        );
        // CR-terminated stream: same clamp semantics.
        assert_eq!(
            VideoProcessor::parse_progress_percent("time=01:00:01.00\r", 60.0),
            100
        );
    }

    // ---------- integration tests (fake sidecars) ----------

    #[test]
    #[cfg(unix)]
    fn duration_mismatch_rejects_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = fake_ffmpeg(&dir.path(), 100, 100, "  *) size=100 ;;");
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["0.9", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor.compress_to_target(
            &input,
            &dir.path().join("out.mp4"),
            50000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
            &tmp,
        );

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("duration"),
            "a 10% shorter output must be rejected: {err}"
        );
        assert!(!dir.path().join("out.mp4").exists());
    }

    #[test]
    #[cfg(unix)]
    fn audio_count_matches_input_unless_drop_is_granted() {
        // Two audio tracks in, two out: the attempt is valid.
        {
            let dir = tempfile::tempdir().unwrap();
            let ffmpeg = fake_ffmpeg(&dir.path(), 100, 100, "  *) size=100 ;;");
            let streams = ["1.0", "video", "audio", "audio"];
            let ffprobe = fake_ffprobe(&dir.path(), &streams, &streams);
            let input = oversized_input(&dir.path(), "in.mp4");
            let tmp = temp_dir_under(&dir.path());

            let result = VideoProcessor
                .compress_to_target(
                    &input,
                    &dir.path().join("out.mp4"),
                    50000,
                    &QualityFloors::default(),
                    CancellationToken::new(),
                    ProgressSender::test_stub(),
                    &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                    &tmp,
                )
                .unwrap();
            assert!(result.met_target, "got: {}", result.strategy_used);
        }

        // One audio track out of two is a silent drop. Task 10 wires the
        // DropStream grant; until then the input count is the floor.
        {
            let dir = tempfile::tempdir().unwrap();
            let ffmpeg = fake_ffmpeg(&dir.path(), 100, 100, "  *) size=100 ;;");
            let ffprobe = fake_ffprobe(
                &dir.path(),
                &["1.0", "video", "audio", "audio", "subtitle"],
                &["1.0", "video", "audio", "subtitle"],
            );
            let input = oversized_input(&dir.path(), "in.mp4");
            let tmp = temp_dir_under(&dir.path());

            let result = VideoProcessor.compress_to_target(
                &input,
                &dir.path().join("out.mp4"),
                50000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                &tmp,
            );

            let err = result.unwrap_err();
            assert!(
                err.to_string().contains("audio"),
                "a dropped audio track must fail validation: {err}"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn subtitle_count_must_match_input() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = fake_ffmpeg(&dir.path(), 100, 100, "  *) size=100 ;;");
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio", "subtitle"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor.compress_to_target(
            &input,
            &dir.path().join("out.mp4"),
            50000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
            &tmp,
        );

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("subtitle"),
            "a dropped subtitle track must fail validation: {err}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn video_over_target_steps_resolution_1080_720_480() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = fake_ffmpeg(&dir.path(), 90000, 90000, "  *) size=100 ;;");
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.mp4"),
                50000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target, "strategy: {}", result.strategy_used);
        assert!(result.strategy_used.contains("crf=23"), "got: {}", result.strategy_used);
        assert!(result.strategy_used.contains("scale=1280"), "got: {}", result.strategy_used);

        let log = ffmpeg_log(&dir.path());
        // One ABR rung (both passes) at 1080p, then the first CRF rung fits.
        assert_eq!(log.len(), 3, "log: {log:?}");
        let count = |needle: &str| log.iter().filter(|l| l.contains(needle)).count();
        assert_eq!(count("scale=min(iw\\,1920)"), 2, "ABR runs only at 1080p: {log:?}");
        assert_eq!(count("scale=min(iw\\,1280)"), 1, "first CRF rung at 720p: {log:?}");
        assert_eq!(count("scale=min(iw\\,854)"), 0, "nothing reached 480p: {log:?}");
        assert_eq!(count("-b:v"), 2, "one ABR encode only: {log:?}");
        // Passlog isolation: the two passes share one prefix.
        let prefixes: Vec<String> = log
            .iter()
            .filter_map(|l| {
                let idx = l.find("-passlogfile ")?;
                Some(l[idx + "-passlogfile ".len()..].split(' ').next().unwrap().to_string())
            })
            .collect();
        let unique: std::collections::HashSet<&String> = prefixes.iter().collect();
        assert_eq!(unique.len(), 1, "one fresh prefix per ABR attempt: {prefixes:?}");
        assert_eq!(prefixes[0], prefixes[1], "pass 2 reuses pass 1's prefix");
        // All passlogs cleaned up after the run.
        let leftovers: Vec<_> = fs::read_dir(&tmp)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("passlog-"))
            .collect();
        assert!(leftovers.is_empty(), "orphan passlogs: {leftovers:?}");
    }

    #[test]
    #[cfg(unix)]
    fn video_crf_floor_28_best_effort() {
        let dir = tempfile::tempdir().unwrap();
        // CRF 23 → 600, CRF 28 → 800: the LAST attempt is NOT the smallest,
        // so last-wins selection would regress this test.
        let ffmpeg = fake_ffmpeg(&dir.path(), 9000, 9000, "  23) size=600 ;;\n    28) size=800 ;;");
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.mp4"),
                500,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                &tmp,
            )
            .unwrap();

        assert!(!result.met_target);
        assert_eq!(result.compressed_size, 600, "smallest output must win");
        assert!(result.strategy_used.contains("crf=23"), "got: {}", result.strategy_used);
        assert!(result.strategy_used.contains("scale=1280"), "got: {}", result.strategy_used);
        assert!(result.strategy_used.contains("best-effort"), "got: {}", result.strategy_used);

        // Floor respected: no CRF above 28 ever reaches ffmpeg.
        let log = ffmpeg_log(&dir.path());
        for line in &log {
            if line.contains("-crf") {
                let crf: u32 = line
                    .split("-crf ")
                    .nth(1)
                    .unwrap()
                    .split(' ')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                assert!(crf <= 28, "CRF {crf} exceeds floor 28");
            }
        }
        assert!(log.iter().any(|l| l.contains("-crf 28")), "floor rung must be tried: {log:?}");
        // Target is too small to fund the 50 kbps ABR floor: only CRF runs,
        // at 720p then 480p, each at both CRF values.
        assert_eq!(log.len(), 4, "log: {log:?}");
        assert_eq!(log.iter().filter(|l| l.contains("scale=min(iw\\,1280)")).count(), 2);
        assert_eq!(log.iter().filter(|l| l.contains("scale=min(iw\\,854)")).count(), 2);
        assert_eq!(log.iter().filter(|l| l.contains("-b:v")).count(), 0, "{log:?}");
    }

    #[test]
    #[cfg(unix)]
    fn video_cancel_mid_encode_kills_ffmpeg_and_removes_passlogs() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = fake_ffmpeg_sleeper(&dir.path());
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let token = CancellationToken::new();
        let output_path = dir.path().join("out.mp4");
        let started = Instant::now();
        let handle = std::thread::spawn({
            let token = token.clone();
            let ffmpeg = ffmpeg.clone();
            let ffprobe = ffprobe.clone();
            let input = input.clone();
            let tmp = tmp.clone();
            move || {
                VideoProcessor.compress_to_target(
                    &input,
                    &output_path,
                    50000,
                    &QualityFloors::default(),
                    token,
                    ProgressSender::test_stub(),
                    &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                    &tmp,
                )
            }
        });

        // Wait until pass 1 has actually spawned (the sleeper logs its
        // invocation immediately) so the cancel is guaranteed to land
        // mid-encode; a fixed sleep races thread startup under load.
        let mut spawned = false;
        for _ in 0..400 {
            if !ffmpeg_log(&dir.path()).is_empty() {
                spawned = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(spawned, "pass 1 never started");
        token.cancel();
        let result = handle.join().unwrap();
        // The fake sleeps 60s in pass 1; returning near-instantly after the
        // cancel proves the child was SIGKILLed, not waited out (an orphan
        // sleeper would hang CI instead of failing loudly).
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "cancel must kill the sleeper child, elapsed: {:?}",
            started.elapsed()
        );

        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "got: {result:?}"
        );
        // Cancel fired mid-pass-1: exactly ONE ffmpeg invocation was made.
        let log = ffmpeg_log(&dir.path());
        assert_eq!(log.len(), 1, "pass 2 must never spawn after cancel: {log:?}");
        // Temp dir must be EMPTY — no orphan passlogs, no partial attempts.
        let leftovers: Vec<_> = fs::read_dir(&tmp).unwrap().flatten().collect();
        assert!(leftovers.is_empty(), "orphaned temp files: {leftovers:?}");
    }

    #[test]
    #[cfg(unix)]
    fn video_mov_input_delivers_queue_resolved_mp4_output() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = fake_ffmpeg(&dir.path(), 100, 100, "  *) size=100 ;;");
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "clip.mov");
        let tmp = temp_dir_under(&dir.path());

        // queue.rs output_file_name resolves clip.mov → clip.mp4 (with
        // collision suffixes) BEFORE the processor runs; the processor must
        // deliver to the resolved path untouched. See
        // queue::output_file_name_rewrites_mov_to_mp4 and
        // queue::mov_collision_uses_real_output_name for the naming half.
        let output = dir.path().join("clip.mp4");

        let result = VideoProcessor
            .compress_to_target(
                &input,
                &output,
                50000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target);
        assert_eq!(result.output_path, output, "processor must not re-derive the output name");
        assert_eq!(result.output_path.extension().unwrap(), "mp4");
        assert!(result.output_path.exists());
    }

    #[test]
    #[cfg(unix)]
    fn video_ffprobe_nonzero_exit_fails_with_engine_error() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = fake_ffmpeg(&dir.path(), 100, 100, "");
        let ffprobe = fake_ffprobe_failing(&dir.path(), "Invalid data found when processing input");
        let input = oversized_input(&dir.path(), "broken.mp4");
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor.compress_to_target(
            &input,
            &dir.path().join("out.mp4"),
            50000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
            &tmp,
        );

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("File may be corrupt"),
            "got: {err}"
        );
        // ffmpeg must never spawn when ffprobe already failed.
        assert!(!dir.path().join("ffmpeg.log").exists());
    }

    #[test]
    #[cfg(unix)]
    fn video_already_under_target_skips_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("small.mp4");
        fs::write(&input, vec![0u8; 1000]).unwrap();
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor
            .compress_to_target(
                &input,
                &dir.path().join("small.mp4"),
                50000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                // None sidecars: any spawn attempt would fail the job.
                &test_sidecar_paths(None, None),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target);
        assert!(result.is_already_under_target());
        assert_eq!(result.compressed_size, 1000);
    }

    #[test]
    #[cfg(unix)]
    fn video_ffprobe_na_duration_fails_job() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = fake_ffmpeg(&dir.path(), 100, 100, "");
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["N/A", "video", "audio"],
            &["N/A", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor.compress_to_target(
            &input,
            &dir.path().join("out.mp4"),
            50000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
            &tmp,
        );

        let err = result.unwrap_err();
        assert!(err.to_string().contains("File may be corrupt"), "got: {err}");
    }

    #[test]
    #[cfg(unix)]
    fn video_missing_sidecar_fails_fast() {
        let dir = tempfile::tempdir().unwrap();
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor.compress_to_target(
            &input,
            &dir.path().join("out.mp4"),
            50000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(None, Some(ffprobe)),
            &tmp,
        );

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Engine error: ffmpeg not available"),
            "got: {err}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn video_ladder_respects_settings_quality_floors() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = fake_ffmpeg(&dir.path(), 90000, 90000, "  *) size=100 ;;");
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let floors = QualityFloors {
            video: crate::settings::VideoQualityFloor {
                min_crf: 23,
                min_resolution: "720p".to_string(),
            },
            ..QualityFloors::default()
        };

        let result = VideoProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.mp4"),
                50000,
                &floors,
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                &tmp,
            )
            .unwrap();

        let log = ffmpeg_log(&dir.path());
        // 720p floor: no 480p (854) attempt may appear; ABR at 1080p then
        // the first CRF rung at 720p fits.
        assert_eq!(log.len(), 3, "log: {log:?}");
        assert_eq!(log.iter().filter(|l| l.contains("scale=min(iw\\,854)")).count(), 0);
        assert_eq!(log.iter().filter(|l| l.contains("scale=min(iw\\,1920)")).count(), 2);
        assert_eq!(log.iter().filter(|l| l.contains("scale=min(iw\\,1280)")).count(), 1);
        assert!(result.strategy_used.contains("crf=23"));
        assert!(result.strategy_used.contains("scale=1280"));
        // CRF floor 23: only rung 23, never 28.
        for line in &log {
            if line.contains("-crf") {
                let crf: u32 = line
                    .split("-crf ")
                    .nth(1)
                    .unwrap()
                    .split(' ')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                assert!(crf <= 23, "CRF {crf} exceeds floor 23");
            }
        }
    }

    #[test]
    fn video_windows_nul_dev_null_gated() {
        #[cfg(windows)]
        assert_eq!(PASS1_SINK, "NUL");
        #[cfg(not(windows))]
        assert_eq!(PASS1_SINK, "/dev/null");
    }

    /// Review round 1: a failing attempt (non-zero ffmpeg exit, or a corrupt
    /// output behind exit 0) must fall through to later rungs instead of
    /// killing the job, and the failed attempt's temp file must be removed.
    #[test]
    #[cfg(unix)]
    fn video_failed_first_rung_falls_through_to_later_rungs() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = make_executable_script(
            &dir.path(),
            "fake-ffmpeg",
            r#"#!/bin/bash
echo "ffmpeg $*" >> "$(dirname "$0")/ffmpeg.log"
pass=""; pl=""; w=""; crf=""
prev=""
for a in "$@"; do
  case "$prev" in
    -pass) pass="$a" ;;
    -passlogfile) pl="$a" ;;
    -vf) w="${a#scale=}"; w="${w%%:*}" ;;
    -crf) crf="$a" ;;
  esac
  prev="$a"
done
if [ "$w" = "min(iw\,1920)" ]; then
  echo "x264: crash at $w" >&2
  exit 1
fi
if [ "$pass" = "1" ]; then
  if [ -n "$pl" ]; then printf 'x' > "${pl}-0.log"; printf 'x' > "${pl}-0.log.mbtree"; fi
  exit 0
fi
out="${@: -1}"
size=9000
if [ -n "$crf" ]; then size=100; fi
printf '\x00\x00\x00\x18ftypmp42' > "$out"
head -c $(( size - 12 )) /dev/zero >> "$out"
"#,
        );
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.mp4"),
                50000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                &tmp,
            )
            .unwrap();

        let log = ffmpeg_log(&dir.path());
        // The 1080p ABR fails on pass 1; the first 720p CRF rung then fits.
        assert_eq!(log.iter().filter(|l| l.contains("min(iw\\,1920)")).count(), 1, "only the failed pass-1: {log:?}");
        assert_eq!(log.iter().filter(|l| l.contains("min(iw\\,1280)")).count(), 1, "{log:?}");
        assert_eq!(log.iter().filter(|l| l.contains("min(iw\\,854)")).count(), 0, "{log:?}");
        assert!(result.met_target, "later rungs must still fit: {}", result.strategy_used);
        assert!(result.strategy_used.contains("crf=23"), "got: {}", result.strategy_used);
        assert!(result.strategy_used.contains("scale=1280"), "got: {}", result.strategy_used);
        // The failed attempt's temp output was removed, not leaked.
        let attempt_files: Vec<String> = fs::read_dir(&tmp)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !attempt_files.iter().any(|n| n == "attempt-0.mp4"),
            "failed attempt-0 output must be removed: {attempt_files:?}"
        );
    }

    /// Review round 1: total failure must surface the LAST attempt's error
    /// (last_error propagation), not a generic message.
    #[test]
    #[cfg(unix)]
    fn video_total_failure_surfaces_last_attempt_error() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = make_executable_script(
            &dir.path(),
            "fake-ffmpeg",
            r#"#!/bin/bash
echo "ffmpeg $*" >> "$(dirname "$0")/ffmpeg.log"
w=""
prev=""
for a in "$@"; do
  case "$prev" in
    -vf) w="${a#scale=}"; w="${w%%:*}" ;;
  esac
  prev="$a"
done
echo "x264: $w exploded" >&2
exit 1
"#,
        );
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor.compress_to_target(
            &input,
            &dir.path().join("out.mp4"),
            50000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
            &tmp,
        );

        let err = result.unwrap_err();
        // The last attempt is the final CRF rung at the 480p cap width.
        assert!(
            err.to_string().contains("min(iw\\,854) exploded"),
            "last_error must carry the final rung's stderr: {err}"
        );
    }

    /// Review round 1: ffmpeg exit 0 is NOT success — a garbage, empty, or
    /// truncated output must reject the attempt and continue the ladder.
    #[test]
    #[cfg(unix)]
    fn video_corrupt_attempt_outputs_rejected_ladder_continues() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = make_executable_script(
            &dir.path(),
            "fake-ffmpeg",
            r#"#!/bin/bash
echo "ffmpeg $*" >> "$(dirname "$0")/ffmpeg.log"
pass=""; pl=""; w=""
prev=""
for a in "$@"; do
  case "$prev" in
    -pass) pass="$a" ;;
    -passlogfile) pl="$a" ;;
    -vf) w="${a#scale=}"; w="${w%%:*}" ;;
  esac
  prev="$a"
done
if [ "$pass" = "1" ]; then
  if [ -n "$pl" ]; then printf 'x' > "${pl}-0.log"; printf 'x' > "${pl}-0.log.mbtree"; fi
  exit 0
fi
out="${@: -1}"
if [ "$w" = "min(iw\,1920)" ]; then
  printf 'GARBAGE-GARBAGE' > "$out"
  exit 0
fi
if [ "$w" = "min(iw\,1280)" ]; then
  : > "$out"
  exit 0
fi
printf '\x00\x00\x00\x18ftypmp42' > "$out"
head -c 88 /dev/zero >> "$out"
"#,
        );
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());

        let result = VideoProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.mp4"),
                50000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                &tmp,
            )
            .unwrap();

        let log = ffmpeg_log(&dir.path());
        // The 1080p ABR emitted garbage behind exit 0; both 720p CRF rungs
        // emitted empty files — all rejected. The 480p CRF rung delivered.
        assert_eq!(log.iter().filter(|l| l.contains("min(iw\\,1920)")).count(), 2, "exit-0 garbage pass must still run: {log:?}");
        assert_eq!(log.iter().filter(|l| l.contains("min(iw\\,1280)")).count(), 2, "{log:?}");
        assert_eq!(log.iter().filter(|l| l.contains("min(iw\\,854)")).count(), 1, "{log:?}");
        assert!(result.met_target, "valid rung must deliver: {}", result.strategy_used);
        assert!(result.strategy_used.contains("scale=854"), "got: {}", result.strategy_used);
        assert_eq!(result.compressed_size, 100);
    }

    /// Review round 1: progress events must be monotonically non-decreasing,
    /// stay within 0..=100, and end at 100 (delivery) on success.
    #[test]
    #[cfg(unix)]
    fn video_progress_monotonic_and_ends_at_100() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = make_executable_script(
            &dir.path(),
            "fake-ffmpeg",
            r#"#!/bin/bash
echo "ffmpeg $*" >> "$(dirname "$0")/ffmpeg.log"
pass=""; pl=""; w=""
prev=""
for a in "$@"; do
  case "$prev" in
    -pass) pass="$a" ;;
    -passlogfile) pl="$a" ;;
    -vf) w="${a#scale=}"; w="${w%%:*}" ;;
  esac
  prev="$a"
done
if [ "$pass" = "1" ]; then
  if [ -n "$pl" ]; then printf 'x' > "${pl}-0.log"; printf 'x' > "${pl}-0.log.mbtree"; fi
  echo "time=00:00:00.40" >&2
  exit 0
fi
echo "time=00:00:00.95" >&2
out="${@: -1}"
printf '\x00\x00\x00\x18ftypmp42' > "$out"
head -c 88 /dev/zero >> "$out"
"#,
        );
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());
        let (progress, seen) = ProgressSender::test_collector();

        let result = VideoProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.mp4"),
                50000,
                &QualityFloors::default(),
                CancellationToken::new(),
                progress,
                &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target);
        let events = seen.lock().unwrap().clone();
        assert!(!events.is_empty(), "progress events must exist");
        assert!(events.iter().all(|&p| (0..=100).contains(&p)), "{events:?}");
        for pair in events.windows(2) {
            assert!(pair[0] <= pair[1], "regressed: {events:?}");
        }
        assert!(*events.last().unwrap() >= 95, "last event must be terminal: {events:?}");
    }

    /// Live progress MUST emit while ffmpeg still runs: real ffmpeg marks
    /// each `-stats` frame update with a carriage return, so a reader that
    /// drains only on `\n` goes silent for the whole encode. The fake
    /// mirrors the real byte pattern — `\r`-terminated stats separated by
    /// sleeps, one `\n` post-exit summary — and drives
    /// `run_ffmpeg_with_progress` directly so pre-encode attempt-base
    /// events cannot confound the observed sequence. Assertions are
    /// sequence-based (not wall-clock): with the bug, 11 and 17 are never
    /// emitted at all; with the fix they arrive during the sleeps.
    #[test]
    #[cfg(unix)]
    fn video_progress_live_during_cr_only_stats() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = make_executable_script(
            &dir.path(),
            "fake-ffmpeg",
            r#"#!/bin/bash
pass=""
sleep 1
printf 'frame= 1 time=00:00:00.30\r' >&2
sleep 1
printf 'frame= 2 time=00:00:00.60\r' >&2
sleep 1
printf 'frame= 3 time=00:00:00.90\r' >&2
sleep 1
echo "frames=3 time=00:00:01.20" >&2
exit 1
"#,
        );
        let (progress, seen) = ProgressSender::test_collector();
        let duration_secs = 5.0;

        let result = VideoProcessor::run_ffmpeg_with_progress(
            &ffmpeg,
            &["-stats".to_string()],
            &CancellationToken::new(),
            &progress,
            0,
            95,
            duration_secs,
        );

        assert!(result.is_err(), "code-1 exit must surface as failure");
        let events = seen.lock().unwrap().clone();
        for pair in events.windows(2) {
            assert!(pair[0] <= pair[1], "regressed: {events:?}");
        }
        // 0.30/5.00=6%→scaled 5; 0.60→11; 0.90→17 — mid-run CR updates.
        for expected in [5, 11, 17] {
            assert!(
                events.contains(&expected),
                "live CR event {expected} missing: {events:?}"
            );
        }
        // Terminating summary drains at EOF and lifts the final value.
        assert_eq!(
            *events.last().unwrap(),
            22,
            "final flush must end at the max parsed percent: {events:?}"
        );
    }

    // ---------- drain_stderr_for_progress (byte-level reader) tests ----------

    #[test]
    fn drain_parses_lf_cr_and_mixed_terminated_lines() {
        // Drain contract: each read ending in \n or \r peels to the LAST
        // terminator; the sink's own lossy lines() walk resolves the
        // individual lines. Real shape: \r-separated stats, one \n summary,
        // trailing \r\n. Nothing may be dropped or duplicated.
        let mut pipe = ChunkedReader::new(&[b"frame= 1 time=00:00:01.00\rframe= 2 time=00:00:02.00\rframes=2\ntime=00:00:03.00\r\n"]);
        let lines = drain_lines(&mut pipe);
        let joined = lines.join("");
        for token in ["time=00:00:01.00", "time=00:00:02.00", "time=00:00:03.00"] {
            assert!(joined.contains(token), "lost {token}: {lines:?}");
        }
        // Feeding the exact drained bytes through the production parser
        // yields the max timestamp (3.00s) — no terminator confusion.
        let max_secs = lines
            .iter()
            .flat_map(|l| l.lines())
            .filter_map(|l| VideoProcessor::progress_line_seconds(l.trim()))
            .fold(0.0f64, f64::max);
        assert_eq!(max_secs, 3.0);
    }

    #[test]
    fn drain_splits_stats_line_across_read_chunks_incl_cr_boundary() {
        // A stats token straddling two read chunks with the CR arriving
        // exactly at the boundary: first read ends mid-token (no terminator),
        // the second delivers the rest + CR. Nothing may parse early; the
        // whole line must peel once its terminator arrives.
        let line = b"frame= 12 time=00:00:24.00 bitrate=600.0kbits/s\r";
        let mut pipe = ChunkedReader::new(&[&line[..20], &line[20..]]);
        let lines = drain_lines(&mut pipe);
        assert_eq!(lines.len(), 1, "one line despite chunk split: {lines:?}");
        // The straddled token survived the chunk boundary intact.
        assert!(
            lines[0].contains("time=00:00:24.00"),
            "token must not be corrupted across chunks: {lines:?}"
        );
    }

    #[test]
    fn drain_parses_multiple_lines_in_one_burst_and_holds_fragment() {
        // One read delivers several lines; drain peels to the last
        // terminator and the sink parses each line individually during the
        // lossy lines() walk. Trailing fragment stays pending until EOF,
        // where it is parsed once rather than dropped.
        let burst = b"a\rframe= 1 time=00:00:09.00\rb\nframe= 2 time=00:00:18.00\nfrag";
        let mut pipe = ChunkedReader::new(&[burst]);
        let lines = drain_lines(&mut pipe);
        assert!(
            lines.iter().flat_map(|l| l.lines()).any(|l| l.contains("time=00:00:09")),
            "first CR line parsed from burst: {lines:?}"
        );
        assert!(
            lines.iter().flat_map(|l| l.lines()).any(|l| l.contains("time=00:00:18")),
            "second LF line parsed from burst: {lines:?}"
        );
        // Terminated portion drained at read time; fragment flushed at EOF
        // as a separate sink invocation.
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines.last().unwrap(), "frag");
    }

    #[test]
    fn drain_sink_receives_lines_in_stream_order_across_chunks() {
        // The line sink is the ONLY parse point: per-line peeling means the
        // sink sees lines in stream order (useful for progress emission).
        let mut pipe = ChunkedReader::new(&[b"time=00:00:02.00\r", b"time=00:00:08.00\r\n"]);
        let lines = drain_lines(&mut pipe);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].contains("00:00:02"), "{lines:?}");
        assert!(lines[1].contains("00:00:08"), "{lines:?}");
    }

    #[test]
    fn drain_dirty_input_is_lossy_not_frozen() {
        // Non-UTF-8 bytes inside a line are replaced, not panic or hang.
        let mut pipe = ChunkedReader::new(&[b"frame= 1 time=00:00:05.00 \xff\xfe junk\r"]);
        let lines = drain_lines(&mut pipe);
        assert!(lines[0].contains("time=00:00:05.00"), "{lines:?}");
    }

    #[test]
    fn drain_retries_when_read_returns_interrupted() {
        // An Interrupted read is transient: the drain loop must retry and
        // keep draining what follows, not drop the rest of the stream.
        let inner = ChunkedReader::new(&[b"time=00:00:01.00\r", b"time=00:00:02.00\r"]);
        let mut pipe = InterruptingReader { inner, fail_on: 1, calls: 0 };
        let lines = drain_lines(&mut pipe);
        assert_eq!(
            lines.len(),
            2,
            "data after Interrupted read must still drain: {lines:?}"
        );
        assert!(lines[0].contains("00:00:01"), "{lines:?}");
        assert!(lines[1].contains("00:00:02"), "{lines:?}");
    }

    #[test]
    fn video_and_generic_paths_share_one_stderr_tail() {
        let src = fs::read_to_string(file!()).unwrap();
        assert!(
            src.contains("StderrTail"),
            "video path must reuse the shared stderr tail"
        );
        let local_tail_constant = "const ".to_string() + "TAIL_KEEP_BYTES:";
        assert!(
            !src.contains(&local_tail_constant),
            "no second tail constant in video.rs"
        );
        let recv_helper = "recv".to_string() + "_stderr_bounded";
        assert!(
            !src.contains(&recv_helper),
            "no private bounded-stderr join helper in video.rs"
        );
    }

    #[test]
    #[cfg(unix)]
    fn progress_is_100_only_after_the_rename() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = make_executable_script(
            &dir.path(),
            "fake-ffmpeg",
            r#"#!/bin/bash
pass=""; pl=""; prev=""
for a in "$@"; do
  case "$prev" in
    -pass) pass="$a" ;;
    -passlogfile) pl="$a" ;;
  esac
  prev="$a"
done
if [ "$pass" = "1" ]; then
  if [ -n "$pl" ]; then printf 'x' > "${pl}-0.log"; printf 'x' > "${pl}-0.log.mbtree"; fi
  exit 0
fi
out="${@: -1}"
printf '\x00\x00\x00\x18ftypmp42' > "$out"
head -c 88 /dev/zero >> "$out"
"#,
        );
        let ffprobe = fake_ffprobe(
            &dir.path(),
            &["1.0", "video", "audio"],
            &["1.0", "video", "audio"],
        );
        let input = oversized_input(&dir.path(), "in.mp4");
        let tmp = temp_dir_under(&dir.path());
        let output = dir.path().join("out.mp4");
        let (progress, seen) = ProgressSender::test_collector();

        let result = VideoProcessor
            .compress_to_target(
                &input,
                &output,
                50000,
                &QualityFloors::default(),
                CancellationToken::new(),
                progress,
                &test_sidecar_paths(Some(ffmpeg), Some(ffprobe)),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target);
        let events = seen.lock().unwrap().clone();
        for (i, &p) in events.iter().enumerate() {
            if p == 100 {
                assert!(
                    output.exists(),
                    "event {i} emitted 100 before dest existed: {events:?}"
                );
            }
        }
        assert_eq!(*events.last().unwrap(), 100, "terminal event must be 100: {events:?}");
    }
}

