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

use tokio_util::sync::CancellationToken;

use crate::engine::result::STRATEGY_ALREADY_UNDER_TARGET;
use crate::engine::{
    BestAttempt, CompressionError, CompressionProcessor, CompressionResult, ProgressSender,
    SidecarName, SidecarPaths, deliver_output, run_sidecar, strict_exit,
};
use crate::models::Category;
use crate::settings::QualityFloors;

pub struct ImageProcessor;

const MAX_SEARCH_ITERATIONS: u32 = 8;
const SCALE_FLOOR_PERCENT: u32 = 50;

impl Default for ImageProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageProcessor {
    pub fn new() -> Self {
        ImageProcessor
    }

    fn get_output_extension(input: &Path) -> String {
        let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext.to_lowercase().as_str() {
            "jpg" | "jpeg" => "jpg".to_string(),
            "png" => "png".to_string(),
            "webp" => "webp".to_string(),
            // HEIC/HEIF → JPEG output (spec §3); unknown → jpg fallback
            _ => "jpg".to_string(),
        }
    }

    /// vips save-string options per output format. `quality` applies only to
    /// lossy formats (jpg/webp). PNG uses palette quantization instead —
    /// pngsave has no lossy Q.
    fn save_options(ext: &str, quality: u32) -> String {
        match ext {
            "png" => "[compression=9,palette,colours=256,strip]".to_string(),
            _ => format!("[Q={},strip]", quality),
        }
    }

    fn run_vips(
        sidecar_path: &str,
        input: &Path,
        output: &Path,
        ext: &str,
        quality: u32,
        cancel_token: &CancellationToken,
    ) -> Result<(), CompressionError> {
        Self::wait_sidecar(
            sidecar_path,
            "vips",
            vec![
                "copy".to_string(),
                input.display().to_string(),
                format!("{}{}", output.display(), Self::save_options(ext, quality)),
            ],
            cancel_token,
        )
    }

    /// Dimension reduction via `vips resize` — scale is a float factor (0.9 = 90%).
    /// NOT `vips thumbnail` (3rd arg there is width in pixels, not a percent).
    fn run_vips_resize(
        sidecar_path: &str,
        input: &Path,
        output: &Path,
        ext: &str,
        quality: u32,
        scale_factor: f64,
        cancel_token: &CancellationToken,
    ) -> Result<(), CompressionError> {
        Self::wait_sidecar(
            sidecar_path,
            "vips resize",
            vec![
                "resize".to_string(),
                input.display().to_string(),
                format!("{}{}", output.display(), Self::save_options(ext, quality)),
                format!("{}", scale_factor),
            ],
            cancel_token,
        )
    }

    /// Reaps a vips sidecar via the shared engine runner: stderr is drained
    /// on a reader thread so a chatty tool can never deadlock on a full
    /// pipe, and the cancel token is polled — a cancel SIGKILLs the child
    /// and surfaces as `Cancelled` (cancelled beats exit code). The earlier
    /// direct-child `wait_with_output` blocked for the child's full runtime,
    /// so a cancel arriving during the final fitting pass was lost (exit 0
    /// → Done without a token re-check); the engine poll loop closes that
    /// gap, and no success path returns without a fresh token check.
    fn wait_sidecar(
        sidecar_path: &str,
        label: &'static str,
        args: Vec<String>,
        cancel_token: &CancellationToken,
    ) -> Result<(), CompressionError> {
        run_sidecar(sidecar_path, label, &args, cancel_token, strict_exit)
    }
}

/// Highest quality whose produced size fits `target_bytes`, within
/// `max_iterations`. Returns None when every attempt stays over target.
/// Pure/closure-injected so the search policy is unit-testable without vips.
fn binary_search_quality(
    lo: u32,
    hi: u32,
    max_iterations: u32,
    target_bytes: u64,
    cancel_token: &CancellationToken,
    progress: &ProgressSender,
    attempt: &mut dyn FnMut(u32) -> Result<(u64, PathBuf), CompressionError>,
) -> Result<Option<(u32, u64, PathBuf)>, CompressionError> {
    debug_assert!(lo <= hi, "binary_search_quality requires lo <= hi");
    let mut lo = lo;
    let mut hi = hi;
    let mut best: Option<(u32, u64, PathBuf)> = None;

    for iteration in 0..max_iterations {
        if cancel_token.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }
        if lo > hi {
            break;
        }

        let mid = (lo + hi) / 2;
        progress.send_progress((iteration * 100 / max_iterations).min(90));

        let (size, path) = attempt(mid)?;
        if size <= target_bytes {
            best = Some((mid, size, path));
            lo = mid + 1;
        } else {
            hi = mid.saturating_sub(1);
        }
    }

    Ok(best)
}

impl CompressionProcessor for ImageProcessor {
    fn category(&self) -> Category {
        Category::Image
    }

    fn supported_extensions(&self) -> &[&str] {
        &["jpg", "jpeg", "png", "webp", "heic", "heif"]
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

        // Already under target — copy as-is (spec §6)
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

        let ext = Self::get_output_extension(input);
        let min_quality = quality_floor.image.min_quality;
        let sidecar_path = sidecar_paths.require(SidecarName::Vips)?;

        // Distinct temp file per attempt — a later attempt must never
        // overwrite the file recorded as the best one.
        let attempt = |n: u32,
                       quality: u32,
                       scale: Option<f64>|
         -> Result<(u64, PathBuf), CompressionError> {
            let temp_file = temp_dir.join(format!("attempt-{n}.{ext}"));
            match scale {
                None => Self::run_vips(
                    sidecar_path,
                    input,
                    &temp_file,
                    &ext,
                    quality,
                    &cancel_token,
                )?,
                Some(s) => Self::run_vips_resize(
                    sidecar_path,
                    input,
                    &temp_file,
                    &ext,
                    quality,
                    s,
                    &cancel_token,
                )?,
            }
            let size = std::fs::metadata(&temp_file)
                .map(|m| m.len())
                .map_err(|e| {
                    CompressionError::OutputFileError(format!(
                        "attempt-{n} ({}): {e}",
                        temp_file.display()
                    ))
                })?;
            // Spec §6: an exit 0 alone is not success — a zero-byte output is
            // a failed attempt, never a Done (0 ≤ target would deliver an
            // empty file). Same class of validation as the PDF processor's
            // output check.
            if size == 0 {
                return Err(CompressionError::SidecarFailed(format!(
                    "vips produced an empty output (attempt-{n})"
                )));
            }
            Ok((size, temp_file))
        };

        // Smallest over-target attempt across quality search + dims search —
        // the best-effort fallback (spec §5 step 7).
        let mut smallest: Option<BestAttempt> = None;
        let mut attempt_n: u32 = 0;

        // PNG: pngsave has no lossy Q, so the quality search would only spawn
        // identical outputs — go straight to dimension reduction at the floor.
        let png_output = ext == "png";

        let quality_search = if png_output {
            None
        } else {
            binary_search_quality(
                min_quality,
                95,
                MAX_SEARCH_ITERATIONS,
                target_bytes,
                &cancel_token,
                &progress,
                &mut |q| {
                    let r = attempt(attempt_n, q, None)?;
                    attempt_n += 1;
                    if r.0 > target_bytes {
                        BestAttempt::keep_smaller_in(
                            &mut smallest,
                            BestAttempt {
                                size: r.0,
                                path: r.1.clone(),
                                // save_options("png") has no Q knob (palette
                                // quantization), so a recorded strategy must
                                // never claim a quality that was not applied.
                                strategy: if png_output {
                                    "palette".to_string()
                                } else {
                                    format!("quality={}", q)
                                },
                                quality: q,
                            },
                        );
                    }
                    Ok(r)
                },
            )?
        };

        if let Some((quality, size, path)) = quality_search {
            return deliver_output(
                &path,
                output,
                size,
                format!("quality={}", quality),
                quality,
                true,
                &progress,
                &cancel_token,
            );
        }

        // Phase 2: dimension reduction in 10% steps, floor at SCALE_FLOOR_PERCENT
        progress.send_progress(92);
        let mut scale_percent = 90;
        while scale_percent >= SCALE_FLOOR_PERCENT {
            if cancel_token.is_cancelled() {
                return Err(CompressionError::Cancelled);
            }
            let scale = scale_percent as f64 / 100.0;
            let r = attempt(attempt_n, min_quality, Some(scale))?;
            attempt_n += 1;

            if r.0 <= target_bytes {
                let strategy_used = if png_output {
                    format!("palette,dims={}%", scale_percent)
                } else {
                    format!("quality={},dims={}%", min_quality, scale_percent)
                };
                return deliver_output(
                    &r.1,
                    output,
                    r.0,
                    strategy_used,
                    min_quality,
                    true,
                    &progress,
                    &cancel_token,
                );
            }
            if r.0 > target_bytes {
                BestAttempt::keep_smaller_in(
                    &mut smallest,
                    BestAttempt {
                        size: r.0,
                        path: r.1.clone(),
                        strategy: if png_output {
                            "palette".to_string()
                        } else {
                            format!("quality={}", min_quality)
                        },
                        quality: min_quality,
                    },
                );
            }
            scale_percent -= 10;
        }

        // Phase 3: best effort — smallest attempt wins (spec §5 step 7)
        if let Some(best) = smallest {
            return deliver_output(
                &best.path,
                output,
                best.size,
                format!("{},best-effort", best.strategy),
                best.quality,
                false,
                &progress,
                &cancel_token,
            );
        }

        Err(CompressionError::SidecarFailed(
            "Could not compress image".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Writes an executable bash fake (chmod 755) — unix only.
    #[cfg(unix)]
    fn make_executable_script(dir: &Path, name: &str, script: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().to_string()
    }

    fn sidecar_paths(path: &str) -> SidecarPaths {
        SidecarPaths {
            vips: Some(path.to_string()),
            ffmpeg: None,
            ffprobe: None,
            gs: None,
            qpdf: None,
            gs_resources: None,
        }
    }

    #[test]
    fn get_output_extension_maps_heic_heif_to_jpg() {
        assert_eq!(
            ImageProcessor::get_output_extension(Path::new("p/photo.heic")),
            "jpg"
        );
        assert_eq!(
            ImageProcessor::get_output_extension(Path::new("p/photo.HEIF")),
            "jpg"
        );
    }

    #[test]
    fn get_output_extension_preserves_png_jpg_webp() {
        assert_eq!(
            ImageProcessor::get_output_extension(Path::new("p/a.png")),
            "png"
        );
        assert_eq!(
            ImageProcessor::get_output_extension(Path::new("p/a.JPG")),
            "jpg"
        );
        assert_eq!(
            ImageProcessor::get_output_extension(Path::new("p/a.webp")),
            "webp"
        );
    }

    #[test]
    fn get_output_extension_defaults_unknown_to_jpg() {
        assert_eq!(
            ImageProcessor::get_output_extension(Path::new("p/a.tiff")),
            "jpg"
        );
        assert_eq!(
            ImageProcessor::get_output_extension(Path::new("p/a")),
            "jpg"
        );
    }

    #[test]
    fn binary_search_finds_highest_quality_meeting_target() {
        // size is a flat 120_000 plateau at Q<=80 and grows 1500 per step
        // above it — exactly Q<=80 fits the target
        let attempt = |q: u32| {
            Ok::<(u64, PathBuf), CompressionError>((
                u64::from(q.max(80)) * 1500,
                PathBuf::from("t"),
            ))
        };
        let result = binary_search_quality(
            30,
            95,
            MAX_SEARCH_ITERATIONS,
            120_000,
            &CancellationToken::new(),
            &ProgressSender::test_stub(),
            &mut |q| attempt(q),
        )
        .unwrap();
        let (q, size, _) = result.expect("should find a fitting quality");
        assert!(size <= 120_000);
        assert!(q >= 80, "expected highest fitting quality, got {}", q);
    }

    #[test]
    fn binary_search_caps_at_max_eight_iterations() {
        // 256-wide window needs exactly 8 halvings to exhaust — verifies the cap
        let count = std::cell::Cell::new(0u32);
        let result = binary_search_quality(
            0,
            255,
            MAX_SEARCH_ITERATIONS,
            1, // target 1 byte — nothing fits
            &CancellationToken::new(),
            &ProgressSender::test_stub(),
            &mut |_q| {
                count.set(count.get() + 1);
                Ok((u64::MAX, PathBuf::from("t")))
            },
        )
        .unwrap();
        assert!(result.is_none());
        assert_eq!(count.get(), 8, "must cap at 8 iterations");
    }

    #[test]
    fn binary_search_exhausts_real_range_within_cap() {
        let count = std::cell::Cell::new(0u32);
        let result = binary_search_quality(
            30,
            95,
            MAX_SEARCH_ITERATIONS,
            1,
            &CancellationToken::new(),
            &ProgressSender::test_stub(),
            &mut |_q| {
                count.set(count.get() + 1);
                Ok((u64::MAX, PathBuf::from("t")))
            },
        )
        .unwrap();
        assert!(result.is_none());
        assert!(count.get() <= 8);
    }

    #[test]
    fn binary_search_cancelled_before_first_iteration_returns_cancelled() {
        let token = CancellationToken::new();
        token.cancel();
        let count = std::cell::Cell::new(0u32);
        let result = binary_search_quality(
            30,
            95,
            MAX_SEARCH_ITERATIONS,
            100,
            &token,
            &ProgressSender::test_stub(),
            &mut |_q| {
                count.set(count.get() + 1);
                Ok((1, PathBuf::from("t")))
            },
        );
        assert!(matches!(result, Err(CompressionError::Cancelled)));
        assert_eq!(count.get(), 0);
    }

    #[test]
    fn binary_search_cancelled_mid_search_returns_cancelled() {
        let token = CancellationToken::new();
        let count = std::cell::Cell::new(0u32);
        let result = binary_search_quality(
            30,
            95,
            MAX_SEARCH_ITERATIONS,
            100,
            &token,
            &ProgressSender::test_stub(),
            &mut |_q| {
                count.set(count.get() + 1);
                if count.get() == 3 {
                    token.cancel();
                }
                Ok((u64::MAX, PathBuf::from("t")))
            },
        );
        assert!(matches!(result, Err(CompressionError::Cancelled)));
    }

    #[test]
    fn already_under_target_copies_file_and_marks_result() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.jpg");
        let output = dir.path().join("out.jpg");
        fs::write(&input, b"tiny").unwrap();

        let result = ImageProcessor
            .compress_to_target(
                &input,
                &output,
                200 * 1024,
                &crate::settings::QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &sidecar_paths("/bin/false"), // must never be invoked
                &dir.path().join("tmp"),
            )
            .unwrap();

        assert!(result.met_target);
        assert_eq!(result.strategy_used, "already-under-target");
        assert_eq!(result.compressed_size, 4);
        assert_eq!(fs::read(&output).unwrap(), b"tiny");
    }

    #[test]
    fn missing_vips_fails_fast_without_temp_litter() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.jpg");
        fs::write(&input, vec![0u8; 300 * 1024]).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let result = ImageProcessor.compress_to_target(
            &input,
            &dir.path().join("out.jpg"),
            200 * 1024,
            &crate::settings::QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &SidecarPaths {
                vips: None,
                ffmpeg: None,
                ffprobe: None,
                gs: None,
                qpdf: None,
                gs_resources: None,
            },
            &tmp,
        );
        match result {
            Err(e) => {
                assert!(
                    matches!(e, CompressionError::SidecarNotFound(ref n) if n == "vips"),
                    "expected SidecarNotFound(vips), got {:?}",
                    e
                );
            }
            Ok(r) => panic!("missing vips must fail, got {:?}", r.strategy_used),
        }
        assert_eq!(
            fs::read_dir(&tmp).unwrap().count(),
            0,
            "no attempt files may be written when the engine is missing"
        );
    }

    #[test]
    fn sidecar_nonzero_exit_marks_job_failed_with_exit_code() {
        // unix only: /bin/false exists and exits 1
        if !Path::new("/bin/false").exists() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.jpg");
        fs::write(&input, vec![0u8; 300 * 1024]).unwrap(); // over target → search runs
        let result = ImageProcessor.compress_to_target(
            &input,
            &dir.path().join("out.jpg"),
            200 * 1024,
            &crate::settings::QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &sidecar_paths("/bin/false"),
            &dir.path().join("tmp"),
        );
        match result {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(msg.contains("exited with code"))
            }
            other => panic!(
                "expected SidecarFailed, got {:?}",
                other.map(|r| r.strategy_used)
            ),
        }
    }

    /// Writes a fake sidecar that prints >STDERR_TAIL_CHARS chars to stderr
    /// ("HEAD_START" + 585 filler + "TAIL_END") and exits 1 — lets the
    /// tail-truncation and cancelled-beats-exit-code branches of
    /// wait_sidecar be pinned without vips.
    #[cfg(unix)]
    fn write_stderr_chatter_failer(dir: &Path) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("chatty-fail");
        let script =
            "#!/bin/bash\nprintf 'HEAD_START%3000d' 0 >&2\nprintf 'TAIL_END\\n' >&2\nexit 1\n";
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    #[cfg(unix)]
    fn wait_sidecar_reports_truncated_stderr_tail_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_stderr_chatter_failer(&dir.path());

        let result = ImageProcessor::wait_sidecar(&fake, "vips", vec![], &CancellationToken::new());
        match result {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(msg.contains("exited with code Some(1)"), "msg: {}", msg);
                assert!(
                    msg.contains("TAIL_END"),
                    "last ~500 chars must be retained: {}",
                    msg
                );
                assert!(
                    !msg.contains("HEAD_START"),
                    "head beyond 500 chars must be truncated: {}",
                    msg
                );
            }
            other => panic!("expected SidecarFailed, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    #[cfg(unix)]
    fn wait_sidecar_cancelled_beats_exit_code() {
        if !Path::new("/bin/false").exists() {
            return;
        }
        let token = CancellationToken::new();
        token.cancel();

        let result = ImageProcessor::wait_sidecar("/bin/false", "vips", vec![], &token);
        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "cancelled must beat the exit code, got {:?}",
            result.err().map(|e| e.to_string())
        );
    }

    /// Writes a deterministic bash fake of the vips CLI:
    /// `copy IN OUT[opts]`         → file of exactly 100_000 bytes
    /// `resize IN OUT[opts] SCALE` → file of exactly 100_000 * SCALE bytes
    /// Every invocation appends `op|save-string` to `invocations.log` next to
    /// the script (lets tests observe which ops ran and pin the save string);
    /// any `.png` save whose options lack `palette` fails with exit 1.
    /// Lets the dims-reduction + best-effort paths be tested end-to-end
    /// without libvips. Skipped on non-unix (needs bash + chmod).
    #[cfg(unix)]
    fn write_fake_vips(dir: &Path) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-vips");
        let script = r#"#!/bin/bash
op="$1"; out="$3"
echo "$op|$3" >> "$(dirname "$0")/invocations.log"
base="${out%%\[*}"
case "$op" in
  copy|resize)
    case "$base" in
      *.png)
        opts="${out#*\[}"; opts="${opts%\]}"
        case ",$opts," in
          *",palette,"*) ;;
          *) echo "png save without palette: $out" >&2; exit 1 ;;
        esac ;;
    esac ;;
esac
case "$op" in
  copy)   head -c 100000 /dev/zero > "$base" ;;
  resize) head -c "$(awk -v s="$4" 'BEGIN { printf "%d", 100000 * s }')" /dev/zero > "$base" ;;
esac
"#;
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    #[cfg(unix)]
    fn quality_floor_reached_triggers_dimension_reduction() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path()); // copy → 100_000 B, resize 0.5 → 50_000 B
        let input = dir.path().join("in.jpg");
        fs::write(&input, vec![0u8; 300 * 1024]).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // target 56320 (55 KB): quality search (always 100_000) fails; resizes
        // 0.9→90k, 0.8→80k, 0.7→70k, 0.6→60k all exceed 56320,
        // resize 0.5 → 50_000 fits
        let result = ImageProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.jpg"),
                55 * 1024,
                &crate::settings::QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &sidecar_paths(&fake),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target);
        assert!(
            result.strategy_used.contains("dims=50%"),
            "got: {}",
            result.strategy_used
        );
        assert_eq!(result.compressed_size, 50_000);
    }

    #[test]
    #[cfg(unix)]
    fn png_output_skips_quality_search_and_fits_via_palette_dims() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path()); // resize 0.6 → 60_000 B
        let input = dir.path().join("in.png");
        fs::write(&input, vec![0u8; 300 * 1024]).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // target 60 KB: palette resize 0.9→0.7 fail, 0.6 → 60_000 ≤ 61_440
        // fits; the quality phase must never run (pngsave has no lossy Q)
        let result = ImageProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.png"),
                60 * 1024,
                &crate::settings::QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &sidecar_paths(&fake),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target);
        let strategy = &result.strategy_used;
        assert!(strategy.contains("palette"), "got: {}", strategy);
        assert!(strategy.contains("dims=60%"), "got: {}", strategy);
        assert_eq!(result.compressed_size, 60_000);

        let log = fs::read_to_string(dir.path().join("invocations.log")).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert!(!lines.is_empty(), "fake sidecar was never invoked");
        assert!(
            lines.iter().all(|l| !l.starts_with("copy|")),
            "quality phase ran for png — short-circuit regressed: {:?}",
            lines
        );
        for line in &lines {
            let save_string = line.split_once('|').expect("log line format").1;
            let opts = save_string.split_once('[').expect("save string options").1;
            let opts = opts.strip_suffix(']').expect("closing bracket");
            assert!(
                opts.contains("palette") && opts.contains("strip"),
                "save string missing palette/strip: {}",
                line
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn unachievable_returns_best_effort_with_smallest_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.jpg");
        fs::write(&input, vec![0u8; 300 * 1024]).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // target 10 KB: nothing ever fits (smallest possible = 50_000)
        let result = ImageProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.jpg"),
                10 * 1024,
                &crate::settings::QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &sidecar_paths(&fake),
                &tmp,
            )
            .unwrap();

        assert!(!result.met_target);
        assert!(result.strategy_used.contains("best-effort"));
        assert_eq!(
            result.compressed_size, 50_000,
            "best effort = smallest attempt, not last quality attempt"
        );
        assert_eq!(
            fs::metadata(&dir.path().join("out.jpg")).unwrap().len(),
            50_000
        );
    }

    #[test]
    #[cfg(unix)]
    fn png_unachievable_best_effort_strategy_claims_palette_not_quality() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path()); // smallest possible resize 0.5 → 50_000 B
        let input = dir.path().join("in.png");
        fs::write(&input, vec![0u8; 300 * 1024]).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // target 10 KB: nothing fits — best effort must describe the actual
        // png save (palette), never a quality value pngsave ignores
        let result = ImageProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.png"),
                10 * 1024,
                &crate::settings::QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &sidecar_paths(&fake),
                &tmp,
            )
            .unwrap();

        assert!(!result.met_target);
        assert!(
            result.strategy_used.contains("palette"),
            "png best-effort strategy must cite the palette save, got: {}",
            result.strategy_used
        );
        assert!(
            !result.strategy_used.contains("quality="),
            "pngsave has no Q knob — a quality=N claim would be false, got: {}",
            result.strategy_used
        );
        assert!(result.strategy_used.contains("best-effort"));
    }

    #[test]
    #[cfg(unix)]
    fn zero_byte_vips_output_is_rejected_not_delivered() {
        let dir = tempfile::tempdir().unwrap();
        let script = r#"#!/bin/bash
op="$1"; out="$3"
base="${out%%\[*}"
: > "$base"
"#;
        let fake = make_executable_script(&dir.path(), "fake-vips", script);
        let input = dir.path().join("in.jpg");
        fs::write(&input, vec![0u8; 300 * 1024]).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // exit 0 + 0-byte output: 0 ≤ target would deliver an empty file as
        // Done — it must be a failed attempt instead (spec §6)
        let result = ImageProcessor.compress_to_target(
            &input,
            &dir.path().join("out.jpg"),
            200 * 1024,
            &crate::settings::QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &sidecar_paths(&fake),
            &tmp,
        );
        match result {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(
                    msg.contains("empty output"),
                    "zero-byte attempt must be rejected, got: {}",
                    msg
                );
            }
            Ok(r) => panic!(
                "zero-byte exit-0 output must never be Done, got {} ({})",
                r.strategy_used, r.compressed_size
            ),
            Err(other) => panic!("expected SidecarFailed, got {:?}", other),
        }
        assert!(
            !dir.path().join("out.jpg").exists(),
            "no empty output may be delivered"
        );
    }

    #[test]
    #[cfg(unix)]
    fn cancel_during_final_fitting_pass_yields_cancelled_not_done() {
        let dir = tempfile::tempdir().unwrap();
        // The final resize (0.4) sleeps 5s before writing its (fitting)
        // output; cancel lands 300ms into that pass. The reaper must poll
        // the token, kill the child, and return Cancelled — never deliver
        // Done for a pass that was cancelled mid-flight.
        let script = r#"#!/bin/bash
op="$1"; out="$3"
base="${out%%\[*}"
case "$op" in
  copy)   head -c 100000 /dev/zero > "$base" ;;
  resize)
    case "$base" in
      *attempt-1*) sleep 5; head -c 20000 /dev/zero > "$base" ;;
      *)           head -c 100000 /dev/zero > "$base" ;;
    esac ;;
esac
"#;
        let fake = make_executable_script(&dir.path(), "fake-vips", script);
        let input = dir.path().join("in.jpg");
        fs::write(&input, vec![0u8; 300 * 1024]).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let token = CancellationToken::new();
        let worker_token = token.clone();
        let worker_input = input.clone();
        let worker_output = dir.path().join("out.jpg");
        let worker_tmp = tmp.clone();
        let worker_fake = fake.clone();

        let handle = std::thread::spawn(move || {
            ImageProcessor.compress_to_target(
                &worker_input,
                &worker_output,
                25 * 1024,
                &crate::settings::QualityFloors::default(),
                worker_token,
                ProgressSender::test_stub(),
                &sidecar_paths(&worker_fake),
                &worker_tmp,
            )
        });
        std::thread::sleep(std::time::Duration::from_millis(500));
        token.cancel();
        let result = handle.join().unwrap();

        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "cancel during the final fitting pass must surface as Cancelled, got {:?}",
            result.map(|r| (r.strategy_used, r.compressed_size))
        );
        assert!(
            !dir.path().join("out.jpg").exists(),
            "no Done delivery after cancel"
        );
    }

    #[test]
    #[cfg(unix)]
    #[ignore = "requires real vips sidecar on this machine (Task 8 real-vips smoke)"]
    fn real_vips_compresses_oversized_jpeg_to_target() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = format!(
            "{}/binaries/{}",
            env!("CARGO_MANIFEST_DIR"),
            crate::sidecar::sidecar_filename("vips").unwrap()
        );

        let input = dir.path().join("noise.jpg");
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let out = std::process::Command::new(&sidecar)
            .args(["gaussnoise"])
            .arg(&input)
            .args(["2000", "2000"])
            .output()
            .expect("gaussnoise generation");
        assert!(out.status.success(), "gaussnoise failed: {:?}", out.stderr);
        let input_size = fs::metadata(&input).unwrap().len();
        assert!(
            input_size > 200 * 1024,
            "noise image unexpectedly small: {} bytes",
            input_size
        );

        let result = ImageProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.jpg"),
                200 * 1024,
                &crate::settings::QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &sidecar_paths(&sidecar),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target, "strategy: {}", result.strategy_used);
        assert!(result.compressed_size <= 200 * 1024);
        assert!(result.strategy_used.starts_with("quality="));
    }
}
