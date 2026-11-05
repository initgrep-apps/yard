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
    deliver_output, run_sidecar, strict_exit, BestAttempt, CompressionError, CompressionProcessor,
    CompressionResult, ProgressSender, SidecarName, SidecarPaths,
};
use crate::models::Category;
use crate::settings::QualityFloors;

pub struct ImageProcessor;

const MAX_SEARCH_ITERATIONS: u32 = 8;
const NO_DOWNSCALE_WIDTH: u32 = 100_000;
const PNG_PALETTE_QUALITY: u32 = 100;
const MAX_HEADER_BYTES: u64 = 1024 * 1024;

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

    fn save_options(ext: &str, quality: u32) -> String {
        match ext {
            "png" => format!("[compression=9,palette,Q={quality},keep=icc]"),
            "webp" => {
                format!("[Q={quality},effort=6,smart-subsample,alpha-q=80,keep=icc]")
            }
            _ => format!("[Q={quality},keep=icc,optimize-coding]"),
        }
    }

    /// One invocation shape for every rung: shrink-on-load thumbnail with
    /// autorotation (on by default) and a fallback sRGB output profile.
    /// `width` is a square target box, so a portrait image's long edge is the
    /// capped dimension too.
    fn attempt_args(input: &Path, output: &Path, width: u32, quality: u32) -> Vec<String> {
        let ext = Self::get_output_extension(input);
        vec![
            "thumbnail".to_string(),
            input.display().to_string(),
            format!("{}{}", output.display(), Self::save_options(&ext, quality)),
            width.to_string(),
            "--size".to_string(),
            "down".to_string(),
            "--output-profile".to_string(),
            "srgb".to_string(),
            "--fail-on".to_string(),
            "error".to_string(),
        ]
    }

    fn run_vips(
        sidecar_path: &str,
        input: &Path,
        output: &Path,
        width: u32,
        quality: u32,
        cancel_token: &CancellationToken,
    ) -> Result<(), CompressionError> {
        Self::wait_sidecar(
            sidecar_path,
            "vips thumbnail",
            Self::attempt_args(input, output, width, quality),
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

/// Pixel rungs: each rung is 0.9x the previous long edge, floored to whole
/// pixels, and no rung may fall below `min_long_edge`.
fn dimension_ladder(long_edge: u32, min_long_edge: u32) -> Vec<u32> {
    let mut rungs = Vec::new();
    let mut edge = long_edge;
    loop {
        let next = edge.saturating_mul(9) / 10;
        if next == 0 || next < min_long_edge {
            break;
        }
        rungs.push(next);
        edge = next;
    }
    rungs
}

/// shortcut: a JPEG whose SOF marker sits past the 1 MiB header window falls
/// back to quality-only (no dimension ladder); add a vipsheader sidecar if
/// such files ever show up.
fn read_dimensions(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_HEADER_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    image_dimensions(&head)
}

/// Reads the stored pixel dimensions. The long edge is orientation-invariant,
/// so this is enough to derive the downscale rungs without a vipsheader
/// sidecar (the bundle ships only `vips`).
fn image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return png_dimensions(data);
    }
    if data.starts_with(&[0xFF, 0xD8]) {
        return jpeg_dimensions(data);
    }
    if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        return webp_dimensions(data);
    }
    if data.get(4..8) == Some(b"ftyp") {
        return heif_dimensions(data);
    }
    None
}

fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.get(12..16) != Some(b"IHDR") {
        return None;
    }
    Some((
        u32::from_be_bytes(data.get(16..20)?.try_into().ok()?),
        u32::from_be_bytes(data.get(20..24)?.try_into().ok()?),
    ))
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let mut marker = data[i + 1];
        i += 2;
        while marker == 0xFF {
            marker = *data.get(i)?;
            i += 1;
        }
        match marker {
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes([*data.get(i)?, *data.get(i + 1)?]));
        if matches!(
            marker,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
        ) {
            let height = u32::from(u16::from_be_bytes([*data.get(i + 3)?, *data.get(i + 4)?]));
            let width = u32::from(u16::from_be_bytes([*data.get(i + 5)?, *data.get(i + 6)?]));
            return Some((width, height));
        }
        i += length;
    }
    None
}

fn webp_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let mut i = 12;
    while i + 8 <= data.len() {
        let fourcc = data.get(i..i + 4)?;
        let size = u32::from_le_bytes(data.get(i + 4..i + 8)?.try_into().ok()?) as usize;
        let chunk = data.get(i + 8..)?;
        match fourcc {
            b"VP8 " => {
                if chunk.get(3..6) != Some(&[0x9D, 0x01, 0x2A]) {
                    return None;
                }
                let width =
                    u32::from(u16::from_le_bytes(chunk.get(6..8)?.try_into().ok()?)) & 0x3FFF;
                let height =
                    u32::from(u16::from_le_bytes(chunk.get(8..10)?.try_into().ok()?)) & 0x3FFF;
                return Some((width, height));
            }
            b"VP8L" => {
                if chunk.first() != Some(&0x2F) {
                    return None;
                }
                let b1 = u32::from(*chunk.get(1)?);
                let b2 = u32::from(*chunk.get(2)?);
                let b3 = u32::from(*chunk.get(3)?);
                let b4 = u32::from(*chunk.get(4)?);
                let width = 1 + (((b2 & 0x3F) << 8) | b1);
                let height = 1 + (((b4 & 0x0F) << 10) | (b3 << 2) | ((b2 & 0xC0) >> 6));
                return Some((width, height));
            }
            b"VP8X" => {
                let width = 1 + le_u24(chunk.get(4..7)?);
                let height = 1 + le_u24(chunk.get(7..10)?);
                return Some((width, height));
            }
            _ => {}
        }
        i += 8 + size + (size & 1);
    }
    None
}

fn le_u24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}

fn heif_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let mut best: Option<(u32, u32)> = None;
    let mut i = 0;
    while let Some(offset) = data[i..].windows(4).position(|window| window == b"ispe") {
        let at = i + offset;
        if let (Some(width), Some(height)) = (data.get(at + 8..at + 12), data.get(at + 12..at + 16))
        {
            let dims = (
                u32::from_be_bytes(width.try_into().ok()?),
                u32::from_be_bytes(height.try_into().ok()?),
            );
            if dims.0 > 0 && dims.1 > 0 && dims.0 <= 100_000 && dims.1 <= 100_000 {
                let area = u64::from(dims.0) * u64::from(dims.1);
                if best.map_or(true, |(w, h)| u64::from(w) * u64::from(h) < area) {
                    best = Some(dims);
                }
            }
        }
        i = at + 4;
    }
    best
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
        let png_output = ext == "png";
        let min_quality = quality_floor.image.min_quality;
        let min_long_edge = quality_floor.image.min_long_edge;
        let sidecar_path = sidecar_paths.require(SidecarName::Vips)?;

        // PNG has no lossy Q search; its palette save always runs at the
        // maximum quantisation quality and pays for it with dimensions.
        let save_quality = if png_output {
            PNG_PALETTE_QUALITY
        } else {
            min_quality
        };
        let long_edge = read_dimensions(input).map(|(width, height)| width.max(height));

        // Distinct temp file per attempt — a later attempt must never
        // overwrite the file recorded as the best one.
        let attempt =
            |n: u32, quality: u32, width: u32| -> Result<(u64, PathBuf), CompressionError> {
                let temp_file = temp_dir.join(format!("attempt-{n}.{ext}"));
                Self::run_vips(
                    sidecar_path,
                    input,
                    &temp_file,
                    width,
                    quality,
                    &cancel_token,
                )?;
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
                    let r = attempt(attempt_n, q, NO_DOWNSCALE_WIDTH)?;
                    attempt_n += 1;
                    if r.0 > target_bytes {
                        BestAttempt::keep_smaller_in(
                            &mut smallest,
                            BestAttempt {
                                size: r.0,
                                path: r.1.clone(),
                                strategy: format!("quality={}", q),
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

        // One full-size palette attempt gives PNG a chance to fit without
        // touching dimensions (and the only legal attempt below the floor).
        let mut widths = if png_output {
            vec![NO_DOWNSCALE_WIDTH]
        } else {
            Vec::new()
        };
        if let Some(long_edge) = long_edge {
            widths.extend(dimension_ladder(long_edge, min_long_edge));
        }

        progress.send_progress(92);
        for width in widths {
            if cancel_token.is_cancelled() {
                return Err(CompressionError::Cancelled);
            }
            let r = attempt(attempt_n, save_quality, width)?;
            attempt_n += 1;

            if r.0 <= target_bytes {
                let strategy_used = if png_output {
                    if width == NO_DOWNSCALE_WIDTH {
                        "palette".to_string()
                    } else {
                        format!("palette,dims={}px", width)
                    }
                } else {
                    format!("quality={},dims={}px", save_quality, width)
                };
                return deliver_output(
                    &r.1,
                    output,
                    r.0,
                    strategy_used,
                    save_quality,
                    true,
                    &progress,
                    &cancel_token,
                );
            }
            BestAttempt::keep_smaller_in(
                &mut smallest,
                BestAttempt {
                    size: r.0,
                    path: r.1.clone(),
                    strategy: if png_output {
                        "palette".to_string()
                    } else {
                        format!("quality={}", save_quality)
                    },
                    quality: save_quality,
                },
            );
        }

        // Best effort — smallest attempt wins (spec §5 step 7)
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

    fn write_jpeg_with_dimensions(path: &Path, width: u16, height: u16, size: usize) {
        let mut bytes = vec![0u8; size.max(11)];
        bytes[0] = 0xFF;
        bytes[1] = 0xD8;
        bytes[2] = 0xFF;
        bytes[3] = 0xC0;
        bytes[4] = 0x00;
        bytes[5] = 0x11;
        bytes[6] = 0x08;
        bytes[7..9].copy_from_slice(&height.to_be_bytes());
        bytes[9..11].copy_from_slice(&width.to_be_bytes());
        fs::write(path, bytes).unwrap();
    }

    fn write_png_with_dimensions(path: &Path, width: u32, height: u32, size: usize) {
        let mut bytes = vec![0u8; size.max(24)];
        bytes[..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        fs::write(path, bytes).unwrap();
    }

    /// Writes a deterministic bash fake of the vips CLI:
    /// `thumbnail IN OUT[opts] WIDTH ...` → 100_000 bytes when WIDTH is the
    /// no-downscale probe, else WIDTH * 25 bytes.
    /// Every invocation appends `op|width|save-string` to `invocations.log`
    /// next to the script (lets tests observe which ops ran and pin the save
    /// string); any save whose options lack `keep=icc`, or any `.png` save
    /// without `palette`, fails with exit 1.
    /// Lets the dims-reduction + best-effort paths be tested end-to-end
    /// without libvips. Skipped on non-unix (needs bash + chmod).
    #[cfg(unix)]
    fn write_fake_vips(dir: &Path) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-vips");
        let script = r#"#!/bin/bash
op="$1"; out="$3"; width="${4:-0}"
echo "$op|$width|$out" >> "$(dirname "$0")/invocations.log"
base="${out%%\[*}"
opts="${out#*\[}"; opts="${opts%\]}"
case ",$opts," in
  *",keep=icc,"*) ;;
  *) echo "save string missing keep=icc: $out" >&2; exit 1 ;;
esac
case "$base" in
  *.png)
    case ",$opts," in
      *",palette,"*) ;;
      *) echo "png save without palette: $out" >&2; exit 1 ;;
    esac ;;
esac
case "$op" in
  thumbnail)
    if [ "$width" -ge 100000 ]; then bytes=100000; else bytes=$((width * 25)); fi
    head -c "$bytes" /dev/zero > "$base" ;;
esac
"#;
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn jpeg_args_rotate_convert_to_srgb_and_keep_icc() {
        let args =
            ImageProcessor::attempt_args(Path::new("in.jpg"), Path::new("out.jpg"), 2000, 80);
        assert_eq!(args[0], "thumbnail");
        assert!(args.contains(&"--output-profile".to_string()));
        assert!(args.contains(&"srgb".to_string()));
        assert!(args.contains(&"--size".to_string()));
        assert!(args.contains(&"down".to_string()));
        let out = &args[2];
        assert!(out.contains("keep=icc"), "{out}");
        assert!(out.contains("Q=80"), "{out}");
        assert!(out.contains("optimize-coding"), "{out}");
        assert!(!out.contains("strip"), "{out}");
        assert!(!out.contains("colours"), "{out}");
    }

    #[test]
    fn thumbnail_is_used_for_every_attempt() {
        let quality = ImageProcessor::attempt_args(
            Path::new("in.jpg"),
            Path::new("out.jpg"),
            NO_DOWNSCALE_WIDTH,
            80,
        );
        assert_eq!(quality[0], "thumbnail");
        assert_eq!(quality[3], "100000");
        assert!(quality.contains(&"--fail-on".to_string()));
        assert!(quality.contains(&"error".to_string()));

        let downscale =
            ImageProcessor::attempt_args(Path::new("in.jpg"), Path::new("out.jpg"), 2000, 30);
        assert_eq!(downscale[0], "thumbnail");
        assert_eq!(downscale[3], "2000");
        assert!(
            !downscale.iter().any(|arg| arg == "copy" || arg == "resize"),
            "full decode ops must be gone: {downscale:?}"
        );
    }

    #[test]
    fn webp_args_use_alpha_q_and_effort() {
        let args =
            ImageProcessor::attempt_args(Path::new("in.webp"), Path::new("out.webp"), 2000, 75);
        let out = &args[2];
        assert!(out.contains("alpha-q=80"), "{out}");
        assert!(!out.contains("alpha-quality"), "{out}");
        assert!(out.contains("effort=6"), "{out}");
        assert!(out.contains("smart-subsample"), "{out}");
        assert!(out.contains("keep=icc"), "{out}");
        assert!(out.contains("Q=75"), "{out}");
    }

    #[test]
    fn png_args_drop_the_ignored_colours_option() {
        let args = ImageProcessor::attempt_args(
            Path::new("in.png"),
            Path::new("out.png"),
            2000,
            PNG_PALETTE_QUALITY,
        );
        let out = &args[2];
        assert!(!out.contains("colours"), "{out}");
        assert!(out.contains("compression=9"), "{out}");
        assert!(out.contains("palette"), "{out}");
        assert!(out.contains("keep=icc"), "{out}");
        assert!(!out.contains("strip"), "{out}");
    }

    #[test]
    fn dimension_ladder_is_pixels_not_percent() {
        assert_eq!(
            dimension_ladder(4000, 1280),
            vec![3600, 3240, 2916, 2624, 2361, 2124, 1911, 1719, 1547, 1392]
        );
        assert!(
            dimension_ladder(4000, 1280)
                .iter()
                .all(|width| *width >= 1280),
            "the ladder must stop before the floor"
        );
        assert_eq!(dimension_ladder(4000, 2000).last(), Some(&2124));
        assert!(
            dimension_ladder(1200, 1280).is_empty(),
            "an input already below the floor must not be downscaled"
        );
    }

    #[test]
    fn image_dimensions_reads_jpeg_and_png_headers() {
        let dir = tempfile::tempdir().unwrap();
        let jpeg = dir.path().join("a.jpg");
        write_jpeg_with_dimensions(&jpeg, 4000, 3000, 64);
        assert_eq!(read_dimensions(&jpeg), Some((4000, 3000)));
        let png = dir.path().join("a.png");
        write_png_with_dimensions(&png, 640, 480, 64);
        assert_eq!(read_dimensions(&png), Some((640, 480)));
    }

    #[test]
    #[cfg(unix)]
    fn icc_failure_fails_the_attempt_instead_of_flattening() {
        let dir = tempfile::tempdir().unwrap();
        let script = r#"#!/bin/bash
echo "$3" >> "$(dirname "$0")/invocations.log"
echo "colour transform failed: ICC profile missing" >&2
exit 1
"#;
        let fake = make_executable_script(&dir.path(), "fake-vips", script);
        let input = dir.path().join("in.jpg");
        write_jpeg_with_dimensions(&input, 4000, 3000, 300 * 1024);
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let output = dir.path().join("out.jpg");

        let result = ImageProcessor.compress_to_target(
            &input,
            &output,
            200 * 1024,
            &crate::settings::QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &sidecar_paths(&fake),
            &tmp,
        );

        match result {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(msg.contains("ICC"), "got: {msg}");
            }
            Ok(r) => panic!(
                "an ICC failure must fail the attempt, got {}",
                r.strategy_used
            ),
            Err(other) => panic!("expected SidecarFailed, got {other:?}"),
        }
        assert!(!output.exists(), "no colour-shifted file may be delivered");

        let log = fs::read_to_string(dir.path().join("invocations.log")).unwrap();
        assert!(
            log.contains("keep=icc"),
            "the failed attempt must have asked vips to keep the profile: {log}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn quality_floor_reached_triggers_dimension_reduction() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.jpg");
        write_jpeg_with_dimensions(&input, 4000, 3000, 300 * 1024);
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // target 56320 (55 KB): quality attempts (100_000) fail; the 4000px
        // ladder produces 90k, 81k, 72.9k, 65.6k, 59k, 53.1k — 2124px fits
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
            result.strategy_used.contains("dims=2124px"),
            "got: {}",
            result.strategy_used
        );
        assert_eq!(result.compressed_size, 53_100);
    }

    #[test]
    #[cfg(unix)]
    fn png_output_skips_quality_search_and_fits_via_palette_dims() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.png");
        write_png_with_dimensions(&input, 4000, 3000, 300 * 1024);
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // target 61_440: one full-size palette attempt (100_000) fails; the
        // ladder's 2361px attempt (59_025) fits. No Q search may run.
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
        assert!(strategy.contains("dims=2361px"), "got: {}", strategy);
        assert_eq!(result.compressed_size, 59_025);

        let log = fs::read_to_string(dir.path().join("invocations.log")).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert!(!lines.is_empty(), "fake sidecar was never invoked");
        let widths: Vec<u32> = lines
            .iter()
            .map(|line| line.split('|').nth(1).unwrap().parse().unwrap())
            .collect();
        assert_eq!(
            widths
                .iter()
                .filter(|width| **width == NO_DOWNSCALE_WIDTH)
                .count(),
            1,
            "png must get exactly one full-size palette attempt, not a Q search: {widths:?}"
        );
        for line in &lines {
            let save_string = line.split('|').nth(2).expect("log line format");
            let opts = save_string.split_once('[').expect("save string options").1;
            let opts = opts.strip_suffix(']').expect("closing bracket");
            assert!(
                opts.contains("palette") && opts.contains("keep=icc"),
                "save string missing palette/keep=icc: {}",
                line
            );
            assert!(!opts.contains("strip"), "strip must be gone: {}", line);
        }
    }

    #[test]
    #[cfg(unix)]
    fn unachievable_returns_best_effort_with_smallest_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.jpg");
        write_jpeg_with_dimensions(&input, 4000, 3000, 300 * 1024);
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // target 10 KB: nothing fits; the smallest ladder rung is 1392px
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
            result.compressed_size, 34_800,
            "best effort = smallest attempt, not last quality attempt"
        );
        assert_eq!(
            fs::metadata(&dir.path().join("out.jpg")).unwrap().len(),
            34_800
        );
    }

    #[test]
    #[cfg(unix)]
    fn png_unachievable_best_effort_strategy_claims_palette_not_quality() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.png");
        write_png_with_dimensions(&input, 4000, 3000, 300 * 1024);
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
            "pngsave has no lossy Q knob — a quality=N claim would be false, got: {}",
            result.strategy_used
        );
        assert!(result.strategy_used.contains("best-effort"));
        assert_eq!(result.compressed_size, 34_800);
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
        // The first fitting ladder rung (attempt-6, 3600px) sleeps 5s before
        // writing its 90_000-byte output; cancel lands 500ms into that pass.
        // The reaper must poll the token, kill the child, and return Cancelled
        // — never deliver Done for a pass that was cancelled mid-flight.
        let script = r#"#!/bin/bash
op="$1"; out="$3"
base="${out%%\[*}"
case "$op" in
  thumbnail)
    case "$base" in
      *attempt-6*) sleep 5; head -c 90000 /dev/zero > "$base" ;;
      *)           head -c 100000 /dev/zero > "$base" ;;
    esac ;;
esac
"#;
        let fake = make_executable_script(&dir.path(), "fake-vips", script);
        let input = dir.path().join("in.jpg");
        write_jpeg_with_dimensions(&input, 4000, 3000, 300 * 1024);
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
                95 * 1024,
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

    #[cfg(unix)]
    fn real_vips_path() -> String {
        format!(
            "{}/binaries/{}",
            env!("CARGO_MANIFEST_DIR"),
            crate::sidecar::sidecar_filename("vips").unwrap()
        )
    }

    #[cfg(unix)]
    fn run_real_vips(args: &[&std::ffi::OsStr]) {
        let output = std::process::Command::new(real_vips_path())
            .args(args)
            .output()
            .expect("spawn vips");
        assert!(
            output.status.success(),
            "vips {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(unix)]
    fn jpeg_segments(data: &[u8], marker: u8) -> Vec<&[u8]> {
        let mut segments = Vec::new();
        let mut i = 2;
        while i + 4 <= data.len() && data[i] == 0xFF {
            let kind = data[i + 1];
            if kind == 0xDA {
                break;
            }
            let len = usize::from(u16::from_be_bytes([data[i + 2], data[i + 3]]));
            if i + 2 + len > data.len() {
                break;
            }
            if kind == marker {
                segments.push(&data[i + 4..i + 2 + len]);
            }
            i += 2 + len;
        }
        segments
    }

    #[cfg(unix)]
    fn jpeg_with_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes());

        let mut out = Vec::with_capacity(jpeg.len() + tiff.len() + 10);
        out.extend_from_slice(&jpeg[..2]);
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        out.extend_from_slice(b"Exif\0\0");
        out.extend_from_slice(&tiff);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    #[cfg(unix)]
    fn jpeg_orientation(data: &[u8]) -> Option<u16> {
        let exif = jpeg_segments(data, 0xE1)
            .into_iter()
            .find(|segment| segment.starts_with(b"Exif\0\0"))?;
        let tiff = &exif[6..];
        let little_endian = tiff.starts_with(b"II");
        assert!(
            little_endian || tiff.starts_with(b"MM"),
            "not a TIFF header: {:?}",
            &tiff[..2]
        );
        let at = |offset: usize| -> Option<&[u8]> { tiff.get(offset..offset + 2) };
        let read_u16 = |offset: usize| -> Option<u16> {
            let bytes = at(offset)?;
            Some(if little_endian {
                u16::from_le_bytes([bytes[0], bytes[1]])
            } else {
                u16::from_be_bytes([bytes[0], bytes[1]])
            })
        };
        let read_u32 = |offset: usize| -> Option<u32> {
            let bytes = tiff.get(offset..offset + 4)?;
            Some(if little_endian {
                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            } else {
                u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            })
        };
        let ifd = read_u32(4)? as usize;
        let count = read_u16(ifd)? as usize;
        for entry in 0..count {
            let offset = ifd + 2 + entry * 12;
            if read_u16(offset)? == 0x0112 {
                return read_u16(offset + 8);
            }
        }
        None
    }

    #[cfg(unix)]
    fn icc_profile(data: &[u8]) -> Vec<u8> {
        let mut chunks: Vec<(u8, &[u8])> = jpeg_segments(data, 0xE2)
            .into_iter()
            .filter(|segment| segment.starts_with(b"ICC_PROFILE\0"))
            .map(|segment| (segment[12], &segment[14..]))
            .collect();
        chunks.sort_by_key(|(sequence, _)| *sequence);
        chunks
            .into_iter()
            .flat_map(|(_, chunk)| chunk.iter().copied())
            .collect()
    }

    #[cfg(unix)]
    fn contains_utf16(data: &[u8], text: &str) -> bool {
        let needle: Vec<u8> = text
            .encode_utf16()
            .flat_map(|unit| unit.to_be_bytes())
            .collect();
        data.windows(needle.len()).any(|window| window == needle)
    }

    #[test]
    #[cfg(unix)]
    #[ignore = "requires real vips sidecar on this machine"]
    fn orientation_table_1_to_8_is_applied() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.jpg");
        run_real_vips(&[
            std::ffi::OsStr::new("gaussnoise"),
            base.as_os_str(),
            std::ffi::OsStr::new("1200"),
            std::ffi::OsStr::new("800"),
        ]);
        let bytes = fs::read(&base).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let sidecar = real_vips_path();

        for orientation in 1..=8u16 {
            let input = dir.path().join(format!("in-{orientation}.jpg"));
            fs::write(&input, jpeg_with_orientation(&bytes, orientation)).unwrap();
            let output = dir.path().join(format!("out-{orientation}.jpg"));

            ImageProcessor
                .compress_to_target(
                    &input,
                    &output,
                    60 * 1024,
                    &crate::settings::QualityFloors::default(),
                    CancellationToken::new(),
                    ProgressSender::test_stub(),
                    &sidecar_paths(&sidecar),
                    &tmp,
                )
                .unwrap();

            let expected = if orientation >= 5 {
                (800, 1200)
            } else {
                (1200, 800)
            };
            assert_eq!(
                read_dimensions(&output),
                Some(expected),
                "orientation {orientation} must be rotated upright"
            );
            assert!(
                matches!(
                    jpeg_orientation(&fs::read(&output).unwrap()),
                    None | Some(1)
                ),
                "orientation {orientation} must not survive into the output"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    #[ignore = "requires real vips sidecar and macOS colour profiles"]
    fn display_p3_is_transformed_and_tagged_srgb() {
        let srgb = "/System/Library/ColorSync/Profiles/sRGB Profile.icc";
        let p3 = "/System/Library/ColorSync/Profiles/Display P3.icc";
        if !Path::new(srgb).exists() || !Path::new(p3).exists() {
            eprintln!(
                "skipping display_p3_is_transformed_and_tagged_srgb: system ICC profiles missing"
            );
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let black = dir.path().join("black.v");
        run_real_vips(&[
            std::ffi::OsStr::new("black"),
            black.as_os_str(),
            std::ffi::OsStr::new("120"),
            std::ffi::OsStr::new("80"),
            std::ffi::OsStr::new("--bands"),
            std::ffi::OsStr::new("3"),
        ]);
        let input = dir.path().join("p3.jpg");
        run_real_vips(&[
            std::ffi::OsStr::new("icc_transform"),
            black.as_os_str(),
            input.as_os_str(),
            std::ffi::OsStr::new(p3),
            std::ffi::OsStr::new("--input-profile"),
            std::ffi::OsStr::new(srgb),
        ]);

        let input_size = fs::metadata(&input).unwrap().len();
        let output = dir.path().join("out.jpg");
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let sidecar = real_vips_path();
        ImageProcessor
            .compress_to_target(
                &input,
                &output,
                input_size / 2,
                &crate::settings::QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &sidecar_paths(&sidecar),
                &tmp,
            )
            .unwrap();

        let bytes = fs::read(&output).unwrap();
        let profile = icc_profile(&bytes);
        assert!(!profile.is_empty(), "output must carry an ICC profile");
        assert!(
            contains_utf16(&profile, "sRGB"),
            "output profile must be sRGB, not merely 'some profile'"
        );
        assert!(
            !contains_utf16(&profile, "Display P3"),
            "the Display P3 profile must have been transformed away"
        );
        assert_eq!(read_dimensions(&output), Some((120, 80)));
    }
}
