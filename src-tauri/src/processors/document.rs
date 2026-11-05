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

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::engine::result::STRATEGY_ALREADY_UNDER_TARGET;
use crate::engine::{
    BestAttempt, CompressionError, CompressionProcessor, CompressionResult, ProgressSender,
    SidecarName, SidecarPaths, deliver_output, run_sidecar, strict_exit,
};
use crate::models::Category;
use crate::settings::QualityFloors;

pub struct DocumentProcessor;

const MAX_SEARCH_ITERATIONS: u32 = 8;
const START_QUALITY: u32 = 95;
const DEFLATE_LEVEL: i64 = 9;

/// Exact OOXML media directories — the ONLY parts whose images are
/// recompressed. Near-miss paths (`word/embeddings/`, root `media/`,
/// `word/mediaExtra/`) pass through untouched.
const MEDIA_DIRS: [&str; 3] = ["word/media/", "ppt/media/", "xl/media/"];

/// Disposition of one zip entry inside an OOXML container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaDisposition {
    /// JPEG/PNG inside an exact media dir — recompressed via vips.
    Recompress,
    /// Inside an exact media dir but not JPEG/PNG (EMF/WMF/…) — original
    /// bytes kept, no error (spec §5).
    PassThrough,
    /// Not media at all — original bytes kept.
    NotMedia,
}

impl DocumentProcessor {
    pub fn new() -> Self {
        DocumentProcessor
    }
}

impl Default for DocumentProcessor {
    fn default() -> Self {
        Self::new()
    }
}

struct ZipEntry {
    name: String,
    is_dir: bool,
    content: Vec<u8>,
    disposition: MediaDisposition,
}

fn media_disposition(entry_name: &str) -> MediaDisposition {
    let in_media_dir = MEDIA_DIRS.iter().any(|dir| entry_name.starts_with(dir));
    if !in_media_dir {
        return MediaDisposition::NotMedia;
    }
    let ext = std::path::Path::new(entry_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" => MediaDisposition::Recompress,
        _ => MediaDisposition::PassThrough,
    }
}

fn media_save_options(ext: &str, quality: u32) -> String {
    match ext {
        "png" => "[compression=9,palette,colours=256,strip]".to_string(),
        _ => format!("[Q={},strip]", quality),
    }
}

fn media_ext(entry_name: &str) -> String {
    std::path::Path::new(entry_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default()
}

fn write_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(DEFLATE_LEVEL))
}

fn snapshot_archive(input: &Path) -> Result<Vec<ZipEntry>, CompressionError> {
    const CORRUPT: &str = "File may be corrupt (not a readable OOXML zip container)";
    let file = std::fs::File::open(input)
        .map_err(|e| CompressionError::InputFileError(format!("{}: {e}", input.display())))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        eprintln!(
            "warn: cannot read {} as zip container: {e}",
            input.display()
        );
        CompressionError::SidecarFailed(CORRUPT.to_string())
    })?;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| {
            eprintln!(
                "warn: cannot read entry {index} of {}: {e}",
                input.display()
            );
            CompressionError::SidecarFailed(CORRUPT.to_string())
        })?;
        let name = entry.name().map_err(|e| {
            eprintln!(
                "warn: cannot read entry name {index} of {}: {e}",
                input.display()
            );
            CompressionError::SidecarFailed(CORRUPT.to_string())
        })?.into_owned();
        let is_dir = entry.is_dir();
        let mut content = Vec::new();
        if !is_dir {
            entry.read_to_end(&mut content).map_err(|e| {
                eprintln!(
                    "warn: cannot read zip entry {name} of {}: {e}",
                    input.display()
                );
                CompressionError::SidecarFailed(CORRUPT.to_string())
            })?;
        }
        let disposition = if is_dir {
            MediaDisposition::NotMedia
        } else {
            media_disposition(&name)
        };
        entries.push(ZipEntry {
            name,
            is_dir,
            content,
            disposition,
        });
    }
    Ok(entries)
}

fn write_rezip(
    entries: &[ZipEntry],
    recompressed: &HashMap<usize, Vec<u8>>,
    out_path: &Path,
) -> Result<u64, CompressionError> {
    let file = std::fs::File::create(out_path).map_err(|e| {
        CompressionError::OutputFileError(format!("{}: {e}", out_path.display()))
    })?;
    let mut writer = zip::ZipWriter::new(file);
    for (index, entry) in entries.iter().enumerate() {
        if entry.is_dir {
            writer
                .add_directory(entry.name.clone(), write_options())
                .map_err(|e| CompressionError::OutputFileError(format!("rezip: {e}")))?;
            continue;
        }
        let content = recompressed.get(&index).unwrap_or(&entry.content);
        writer
            .start_file(entry.name.clone(), write_options())
            .map_err(|e| CompressionError::OutputFileError(format!("rezip: {e}")))?;
        writer.write_all(content).map_err(|e| {
            CompressionError::OutputFileError(format!("rezip entry {}: {e}", entry.name))
        })?;
    }
    writer
        .finish()
        .map_err(|e| CompressionError::OutputFileError(format!("rezip finish: {e}")))?;
    std::fs::metadata(out_path)
        .map(|m| m.len())
        .map_err(|e| {
            CompressionError::OutputFileError(format!("{}: {e}", out_path.display()))
        })
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

/// Removes one file, tolerating NotFound; anything else is warned but
/// swallowed — best-effort cleanup must never mask the real result.
fn remove_file_best_effort(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!("warn: could not remove {}: {e}", path.display());
        }
    }
}

/// Delivers a pending output only while the job is still live. Every
/// delivery window (found pass, best effort, zero-media rezip) funnels
/// through here so a cancel that lands after the last sidecar spawn —
/// during the final rezip or between search completion and delivery —
/// cannot produce a Done result.
///
/// Ownership: the output path is resolved free by the queue before the
/// job, but files can appear under it through an external race, so every
/// cleanup choice is gated on a snapshot taken right before the copy:
/// a path that was free pre-copy is fully ours (created or partially
/// written by our deliver) and is removed — cancel or non-cancel
/// failure alike; a path that was occupied pre-copy is never deleted
/// (we cannot un-clobber, and removal could hit foreign data).
fn deliver_or_cancel(
    pending: Result<(PathBuf, u64, String, u32, bool), CompressionError>,
    output: &Path,
    cancel_token: &CancellationToken,
    temp_paths: &[PathBuf],
    progress: &ProgressSender,
) -> Result<CompressionResult, CompressionError> {
    if cancel_token.is_cancelled() {
        remove_all(temp_paths);
        return Err(CompressionError::Cancelled);
    }
    let (path, size, strategy, quality, met_target) = match pending {
        Ok(deliverable) => deliverable,
        Err(e) => {
            remove_all(temp_paths);
            return Err(e);
        }
    };
    let output_was_free = !output.exists();
    let result = deliver_output(&path, output, size, strategy, quality, met_target, progress, cancel_token);
    if result.is_ok() && cancel_token.is_cancelled() {
        remove_all(temp_paths);
        if output_was_free {
            remove_file_best_effort(output);
        } else {
            eprintln!(
                "warn: cancel after delivery: {} was occupied before the copy, left in place",
                output.display()
            );
        }
        return Err(CompressionError::Cancelled);
    }
    if result.is_err() {
        remove_all(temp_paths);
        if output_was_free {
            remove_file_best_effort(output);
        }
    }
    result
}

fn per_media_progress(pass: u32, media_done: usize, media_total: usize) -> u32 {
    let total_units = u64::from(MAX_SEARCH_ITERATIONS) * u64::try_from(media_total).unwrap_or(1);
    let done_units = u64::from(pass) * u64::try_from(media_total).unwrap_or(1)
        + u64::try_from(media_done).unwrap_or(0);
    ((done_units * 100 / total_units) as u32).min(90)
}

fn search_quality(
    lo: u32,
    hi: u32,
    max_iterations: u32,
    target_bytes: u64,
    cancel_token: &CancellationToken,
    attempt: &mut dyn FnMut(u32, u32) -> Result<(u64, PathBuf), CompressionError>,
) -> Result<Option<(u32, u64, PathBuf)>, CompressionError> {
    debug_assert!(lo <= hi, "search_quality requires lo <= hi");
    let mut lo = lo;
    let mut hi = hi;
    let mut best: Option<(u32, u64, PathBuf)> = None;
    let mut pass: u32 = 0;

    for _iteration in 0..max_iterations {
        if cancel_token.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }
        if lo > hi {
            break;
        }
        let mid = (lo + hi) / 2;
        let (size, path) = attempt(pass, mid)?;
        pass += 1;
        if size <= target_bytes {
            best = Some((mid, size, path));
            lo = mid + 1;
        } else {
            hi = mid.saturating_sub(1);
        }
    }
    Ok(best)
}

impl CompressionProcessor for DocumentProcessor {
    fn category(&self) -> Category {
        Category::Document
    }

    fn supported_extensions(&self) -> &[&str] {
        &["docx", "xlsx", "pptx"]
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

        // Already under target — copy as-is, no re-zip churn (spec §6)
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

        let sidecar = sidecar_paths.require(SidecarName::Vips)?;
        // Search window [lo, hi]: the settings floor is the WORST quality
        // we may attempt. A floor at or below START_QUALITY searches the
        // full [floor, START] range. A floor above the start would invert
        // the window (lo > hi → zero attempts), so the search runs as a
        // single attempt pinned exactly AT the floor — the user's minimum
        // quality is honored, never silently relaxed (video CRF walk
        // precedent: exact rung at the floor instead of clamping below it).
        let floor = quality_floor.document.min_media_quality;
        let (min_quality, max_quality) = if floor > START_QUALITY {
            (floor, floor)
        } else {
            (floor, START_QUALITY)
        };
        let ext = std::path::Path::new(input)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_else(|| "docx".to_string());

        let entries = snapshot_archive(input)?;
        let media_total = entries
            .iter()
            .filter(|e| e.disposition == MediaDisposition::Recompress)
            .count();
        let mut temp_paths: Vec<PathBuf> = Vec::new();

        if cancel_token.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }

        // Zero-media document: nothing to recompress — a single pure rezip
        // at DEFLATE 9 (valid zip, entries byte-identical, no sidecar churn).
        if media_total == 0 {
            let attempt_path = temp_dir.join(format!("attempt-0.{ext}"));
            temp_paths.push(attempt_path.clone());
            let pending = write_rezip(&entries, &HashMap::new(), &attempt_path).map(|size| {
                let (met_target, strategy) = if size <= target_bytes {
                    (true, "rezip-deflate9".to_string())
                } else {
                    (false, "rezip-deflate9,best-effort".to_string())
                };
                (attempt_path, size, strategy, 100, met_target)
            });
            return deliver_or_cancel(pending, output, &cancel_token, &temp_paths, &progress);
        }

        // One pass = recompress every JPEG/PNG media entry at the pass
        // quality (a vips failure on one entry keeps that entry's ORIGINAL
        // bytes and continues — the job never fails on a single media), then
        // re-zip everything at DEFLATE level 9. Over-target passes record
        // their output as the best-effort candidate.
        let mut smallest: Option<BestAttempt> = None;
        let mut attempt = |pass: u32, quality: u32| -> Result<(u64, PathBuf), CompressionError> {
            if cancel_token.is_cancelled() {
                return Err(CompressionError::Cancelled);
            }
            let mut recompressed: HashMap<usize, Vec<u8>> = HashMap::new();
            let mut media_done: usize = 0;
            for (index, entry) in entries.iter().enumerate() {
                if entry.disposition != MediaDisposition::Recompress {
                    continue;
                }
                progress.send_progress(per_media_progress(pass, media_done, media_total));
                media_done += 1;

                let media_ext = media_ext(&entry.name);
                let media_in = temp_dir.join(format!("media-{pass}-{index}.{media_ext}"));
                let media_out = temp_dir.join(format!("media-{pass}-{index}-out.{media_ext}"));
                std::fs::write(&media_in, &entry.content).map_err(|e| {
                    CompressionError::OutputFileError(format!("{}: {e}", media_in.display()))
                })?;
                temp_paths.push(media_in.clone());
                temp_paths.push(media_out.clone());

                let args = vec![
                    "copy".to_string(),
                    media_in.display().to_string(),
                    format!(
                        "{}{}",
                        media_out.display(),
                        media_save_options(&media_ext, quality)
                    ),
                ];
                match run_sidecar(sidecar, "vips", &args, &cancel_token, strict_exit) {
                    Ok(()) => match std::fs::read(&media_out) {
                        Ok(bytes) if !bytes.is_empty() => {
                            recompressed.insert(index, bytes);
                        }
                        Ok(_) => {
                            eprintln!(
                                "warn: vips produced empty output for {}, keeping original",
                                entry.name
                            );
                        }
                        Err(e) => {
                            eprintln!(
                                "warn: cannot read recompressed media {}: {e}, keeping original",
                                entry.name
                            );
                        }
                    },
                    Err(CompressionError::Cancelled) => {
                        return Err(CompressionError::Cancelled);
                    }
                    Err(e) => {
                        eprintln!(
                            "warn: vips failed for {}, keeping original bytes: {e}",
                            entry.name
                        );
                    }
                }
            }

            let attempt_path = temp_dir.join(format!("attempt-{pass}.{ext}"));
            temp_paths.push(attempt_path.clone());
            let size = write_rezip(&entries, &recompressed, &attempt_path)?;
            if size > target_bytes {
                BestAttempt::keep_smaller_in(
                    &mut smallest,
                    BestAttempt {
                        size,
                        path: attempt_path.clone(),
                        strategy: format!("quality={quality}"),
                        quality,
                    },
                );
            }
            Ok((size, attempt_path))
        };

        let search = search_quality(
            min_quality,
            max_quality,
            MAX_SEARCH_ITERATIONS,
            target_bytes,
            &cancel_token,
            &mut attempt,
        );

        match search {
            Ok(Some((quality, size, path))) => {
                let pending = if cancel_token.is_cancelled() {
                    Err(CompressionError::Cancelled)
                } else {
                    Ok((path, size, format!("quality={quality}"), quality, true))
                };
                deliver_or_cancel(pending, output, &cancel_token, &temp_paths, &progress)
            }
            Ok(None) => {
                if let Some(best) = smallest {
                    let pending = if cancel_token.is_cancelled() {
                        Err(CompressionError::Cancelled)
                    } else {
                        Ok((
                            best.path,
                            best.size,
                            format!("{},best-effort", best.strategy),
                            best.quality,
                            false,
                        ))
                    };
                    return deliver_or_cancel(
                        pending,
                        output,
                        &cancel_token,
                        &temp_paths,
                        &progress,
                    );
                }
                remove_all(&temp_paths);
                Err(CompressionError::SidecarFailed(
                    "Could not compress document".to_string(),
                ))
            }
            Err(CompressionError::Cancelled) => {
                remove_all(&temp_paths);
                Err(CompressionError::Cancelled)
            }
            Err(e) => {
                remove_all(&temp_paths);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use zip::ZipArchive;
    use zip::ZipWriter;

    #[cfg(unix)]
    fn make_executable_script(dir: &Path, name: &str, script: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().to_string()
    }

    #[cfg(unix)]
    fn test_sidecar_paths(vips: Option<String>) -> SidecarPaths {
        SidecarPaths {
            vips,
            ffmpeg: None,
            ffprobe: None,
            gs: None,
            qpdf: None,
            gs_resources: None,
        }
    }

    /// Builds a DOCX-like zip: entries in the given order, Deflated.
    fn build_docx(path: &Path, entries: &[(&str, Vec<u8>)]) {
        let file = fs::File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        for (name, content) in entries {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap();
    }

    /// Reads the produced zip back: (name, decompressed content) per entry,
    /// in stored order.
    fn read_zip_contents(path: &Path) -> Vec<(String, Vec<u8>)> {
        let file = fs::File::open(path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut out = Vec::with_capacity(archive.len());
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let name = entry.name().unwrap().into_owned();
            let mut content = Vec::new();
            if !entry.is_dir() {
                entry.read_to_end(&mut content).unwrap();
            }
            out.push((name, content));
        }
        out
    }

    /// Compressed size of one entry by name (first match).
    fn entry_compressed_size(path: &Path, name: &str) -> u64 {
        let file = fs::File::open(path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            if entry.name().unwrap().as_ref() == name {
                return entry.compressed_size();
            }
        }
        panic!("entry {name} not found in {}", path.display());
    }

    /// Writes a reference zip at an explicit deflate level (None = crate
    /// default level 6) for the size-equality verification of level 9.
    fn write_reference_zip<S: AsRef<str>>(
        path: &Path,
        entries: &[(S, Vec<u8>)],
        level: Option<i64>,
    ) {
        let file = fs::File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        for (name, content) in entries {
            writer
                .start_file(
                    name.as_ref(),
                    SimpleFileOptions::default()
                        .compression_method(CompressionMethod::Deflated)
                        .compression_level(level),
                )
                .unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap();
    }

    /// Varied but deterministic text — deflate level 9 produces a different
    /// stream size than the default level for this content.
    fn varied_text(lines: usize) -> Vec<u8> {
        let mut s = String::new();
        for i in 0..lines {
            s.push_str(&format!(
                "record-{i:06}: {} padding\n",
                "x".repeat(i % 37)
            ));
        }
        s.into_bytes()
    }

    /// Deterministic incompressible byte stream (Park–Miller LCG, bytes
    /// 1..=255). Deflate gains nothing on it, so pass sizes track the fake
    /// vips output size monotonically.
    fn noise_bytes(n: usize) -> Vec<u8> {
        const A: u64 = 16_807;
        const M: u64 = 2_147_483_647;
        let mut s: u64 = 12_345;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            s = (s.wrapping_mul(A).wrapping_add(11)) % M;
            v.push(((s / 32_768) % 255 + 1) as u8);
        }
        v
    }

    const CONTENT_TYPES: &[u8] =
        b"<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"></Types>";
    const DOCUMENT_XML: &[u8] = b"<?xml version=\"1.0\"?><w:document xmlns:w=\"x\"></w:document>";
    const RELS_XML: &[u8] = b"<?xml version=\"1.0\"?><Relationships></Relationships>";

    /// Fake vips: `copy IN OUT[opts]` writes exactly `q * 100` bytes of the
    /// deterministic LCG noise pattern, where q is parsed from the `[Q=…]`
    /// save string. Logs one `q` per invocation to `q.log` beside the
    /// script.
    #[cfg(unix)]
    fn write_fake_vips(dir: &Path) -> String {
        make_executable_script(
            dir,
            "fake-vips",
            r#"#!/bin/bash
out="$3"
base="${out%%\[*}"
q="${out#*\[Q=}"; q="${q%,*}"; q="${q%%\]*}"
echo "$q" >> "$(dirname "$0")/q.log"
n=$(( q * 100 ))
awk -v n="$n" 'BEGIN { s = 12345; for (i = 0; i < n; i++) { s = (s * 16807 + 11) % 2147483647; printf "%c", int(s / 32768) % 255 + 1 } }' > "$base"
"#,
        )
    }

    #[cfg(unix)]
    fn read_q_log(dir: &Path) -> Vec<u32> {
        fs::read_to_string(dir.join("q.log"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect()
    }

    fn floors_with(min_media_quality: u32) -> QualityFloors {
        QualityFloors {
            document: crate::settings::DocumentQualityFloor {
                min_media_quality,
            },
            ..QualityFloors::default()
        }
    }

    #[test]
    fn document_media_path_identification() {
        // exact media dirs, JPEG/PNG → recompress
        for name in [
            "word/media/image1.png",
            "word/media/image1.jpg",
            "word/media/image1.jpeg",
            "word/media/image1.PNG",
            "ppt/media/pic.jpeg",
            "xl/media/chart.jpg",
        ] {
            assert_eq!(
                media_disposition(name),
                MediaDisposition::Recompress,
                "{name} must be recompressed"
            );
        }
        // exact media dirs, non-JPEG/PNG → pass through untouched
        for name in [
            "word/media/image1.wmf",
            "word/media/image2.emf",
            "ppt/media/icon.tif",
            "xl/media/logo.gif",
        ] {
            assert_eq!(
                media_disposition(name),
                MediaDisposition::PassThrough,
                "{name} must pass through"
            );
        }
        // near-misses and non-media → not media
        for name in [
            "word/embeddings/oleObject1.png",
            "media/image1.png",
            "word/mediaExtra/image1.png",
            "word/medias/image1.png",
            "xl/media.png",
            "[Content_Types].xml",
            "word/document.xml",
            "docProps/core.xml",
            "_rels/.rels",
            "ppt/slides/slide1.xml",
        ] {
            assert_eq!(
                media_disposition(name),
                MediaDisposition::NotMedia,
                "{name} must not be treated as media"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn document_non_media_entries_copied_byte_identical() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("_rels/.rels", RELS_XML.to_vec()),
                ("docProps/core.xml", b"<core>props</core>".to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.jpg", noise_bytes(9000)),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                5120,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target, "strategy: {}", result.strategy_used);

        let output = read_zip_contents(&dir.path().join("out.docx"));
        let names: Vec<&str> = output.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "[Content_Types].xml",
                "_rels/.rels",
                "docProps/core.xml",
                "word/document.xml",
                "word/media/image1.jpg",
            ],
            "entry order must be preserved with [Content_Types].xml first"
        );
        assert_eq!(output[0].1.as_slice(), CONTENT_TYPES);
        assert_eq!(output[1].1.as_slice(), RELS_XML);
        assert_eq!(output[2].1, b"<core>props</core>".to_vec());
        assert_eq!(output[3].1.as_slice(), DOCUMENT_XML);
    }

    #[cfg(unix)]
    #[test]
    fn document_rezip_uses_deflate_level_9() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        let doc_xml = varied_text(4000);
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", doc_xml.clone()),
                ("word/media/image1.jpg", noise_bytes(9000)),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // Target 100: unreachable — the pass still runs and the produced
        // (best-effort) zip is what gets verified for method + level.
        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                100,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();
        assert!(!result.met_target, "strategy: {}", result.strategy_used);

        let out_path = dir.path().join("out.docx");
        let produced = read_zip_contents(&out_path);

        // Every entry: Deflated method.
        let file = fs::File::open(&out_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            assert_eq!(
                entry.compression(),
                CompressionMethod::Deflated,
                "entry {} must be deflated",
                entry.name().unwrap()
            );
        }

        // Level verification: produced compressed sizes must equal a
        // level-9 reference for identical content — and differ from the
        // default (level 6) reference for at least the varied text entry.
        let produced_entries: Vec<(String, Vec<u8>)> = produced
            .iter()
            .map(|(n, c)| (n.clone(), c.clone()))
            .collect();
        let ref9 = dir.path().join("ref9.zip");
        let ref6 = dir.path().join("ref6.zip");
        write_reference_zip(&ref9, &produced_entries, Some(DEFLATE_LEVEL));
        write_reference_zip(&ref6, &produced_entries, None);

        for (name, _) in &produced_entries {
            let produced_size = entry_compressed_size(&out_path, name);
            let level9_size = entry_compressed_size(&ref9, name);
            assert_eq!(
                produced_size, level9_size,
                "entry {name} must be deflated at level 9"
            );
        }
        let produced_doc = entry_compressed_size(&out_path, "word/document.xml");
        let level6_doc = entry_compressed_size(&ref6, "word/document.xml");
        assert_ne!(
            produced_doc, level6_doc,
            "varied text must compress differently at level 9 vs 6 — else this test proves nothing"
        );
    }

    #[cfg(unix)]
    #[test]
    fn document_media_binary_search_floors_at_q40() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.jpg", noise_bytes(8000)),
            ],
        );

        // Default floor 40 — search must descend to exactly Q=40.
        let tmp = dir.path().join("tmp40");
        fs::create_dir_all(&tmp).unwrap();
        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out40.docx"),
                100, // nothing ever fits — search descends fully
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake.clone())),
                &tmp,
            )
            .unwrap();
        assert!(!result.met_target);
        let q40_run = read_q_log(&dir.path());
        assert!(
            q40_run.contains(&40),
            "search must reach the settings floor 40, got {q40_run:?}"
        );
        assert!(
            q40_run.iter().all(|q| (40..=95).contains(q)),
            "no quality below the floor may be attempted, got {q40_run:?}"
        );

        // Custom floor 10 from settings — a lower floor must be honored,
        // proving the floor is wired from settings, not hardcoded.
        fs::remove_file(dir.path().join("q.log")).unwrap();
        let tmp = dir.path().join("tmp10");
        fs::create_dir_all(&tmp).unwrap();
        DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out10.docx"),
                100,
                &floors_with(10),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();
        let q10_run = read_q_log(&dir.path());
        assert!(
            q10_run.contains(&10),
            "custom floor 10 must be honored, got {q10_run:?}"
        );
        let min10 = *q10_run.iter().min().unwrap();
        let min40 = *q40_run.iter().min().unwrap();
        assert!(
            min10 < min40,
            "floor must come from settings: floor10 run min {min10} must be < floor40 run min {min40}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn document_floor_above_start_quality_clamped_to_one_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.jpg", noise_bytes(8000)),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // Floor 99 exceeds START_QUALITY 95 — the search must not relax the
        // floor (that would violate the user's minimum). Instead the window
        // is the single point [99, 99]: exactly one attempt AT the floor,
        // a best-effort delivery, never a zero-attempt collapse or a clamp
        // below the user's floor.
        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                100,
                &floors_with(99),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        assert!(!result.met_target, "strategy: {}", result.strategy_used);
        let log = read_q_log(&dir.path());
        assert_eq!(
            log,
            vec![99],
            "floor 99 must pin exactly one attempt at q=99 (the floor itself), got {log:?}"
        );
        assert_eq!(
            result.quality_used, 99,
            "delivered quality must be the floor, not the clamped start"
        );
        assert!(
            result.strategy_used.contains("best-effort"),
            "strategy: {}",
            result.strategy_used
        );
    }

    #[cfg(unix)]
    #[test]
    fn document_over_target_lowers_quality_and_retries() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.jpg", noise_bytes(8000)),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // Target sits between the highest attempt (q=67 → 6.7 KB noise,
        // incompressible → rezip overshoots) and the floor attempt (q=40 →
        // 4 KB fits), so the search must lower quality and retry.
        let target = 5500;

        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                target,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target, "strategy: {}", result.strategy_used);
        let strategy_quality: u32 = result
            .strategy_used
            .strip_prefix("quality=")
            .expect("strategy must name the quality")
            .parse()
            .expect("strategy quality must be numeric");

        let log = read_q_log(&dir.path());
        assert!(
            log.len() >= 2,
            "over-target first attempt must be retried, got {log:?}"
        );
        assert_eq!(
            log[0], 67,
            "first attempt must be the midpoint of the [floor,95] window"
        );
        assert!(
            log[1] < log[0],
            "after an over-target attempt quality must be lowered, got {log:?}"
        );
        assert!(
            log.iter().all(|q| (40..=95).contains(q)),
            "attempted qualities must stay within [floor, start], got {log:?}"
        );

        // The delivered output must carry media recompressed at exactly the
        // delivered strategy quality: decompressed media bytes == q * 100.
        let output = read_zip_contents(&dir.path().join("out.docx"));
        let media_entry = output
            .iter()
            .find(|(n, _)| n == "word/media/image1.jpg")
            .expect("media entry must survive");
        assert_eq!(
            media_entry.1,
            noise_bytes(usize::try_from(strategy_quality).unwrap() * 100),
            "output media must be recompressed at the delivered quality"
        );
        assert!(
            result.compressed_size <= target,
            "delivered size {} must fit target {target}",
            result.compressed_size
        );
    }

    #[cfg(unix)]
    #[test]
    fn document_emf_wmf_skipped_without_error() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.wmf", vec![1, 2, 3, 4, 5]),
                ("word/media/image2.jpg", noise_bytes(8000)),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                6144,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        assert!(
            result.met_target,
            "mixed WMF+JPEG doc must still be Done, strategy: {}",
            result.strategy_used
        );
        assert!(
            !read_q_log(&dir.path()).is_empty(),
            "vips must be invoked for the JPEG media"
        );
        let output = read_zip_contents(&dir.path().join("out.docx"));
        assert_eq!(output.len(), 4, "no entry may be dropped");
        let wmf_entry = output.iter().find(|(n, _)| n == "word/media/image1.wmf");
        assert_eq!(
            wmf_entry.map(|(_, c)| c.as_slice()),
            Some(&[1u8, 2, 3, 4, 5][..]),
            "WMF media must pass through byte-identical, untouched"
        );
    }

    #[cfg(unix)]
    #[test]
    fn document_vips_fails_on_media_copies_original_entry() {
        let dir = tempfile::tempdir().unwrap();
        let fake = make_executable_script(
            dir.path(),
            "fake-vips-fail",
            r#"#!/bin/bash
out="$3"
case "$out" in
  *"[Q="*) q="${out#*\[Q=}"; q="${q%,*}" ;;
  *) q="png" ;;
esac
echo "$q" >> "$(dirname "$0")/q.log"
echo "simulated vips failure" >&2
exit 1
"#,
        );
        let original_media = noise_bytes(8000);
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.png", original_media.clone()),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // .unwrap() here IS the "job not failed" assertion: a single failed
        // media recompression must surface neither Err nor a dropped entry —
        // the original bytes are kept and re-zipped.
        DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                6144,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        // PNG saves carry no [Q=…] option — assert on the raw invocation
        // log, which the fake writes for every spawn regardless of shape.
        let raw_log = fs::read_to_string(dir.path().join("q.log")).unwrap_or_default();
        assert!(
            !raw_log.is_empty(),
            "vips must have been invoked (and failed) for the media"
        );
        let output = read_zip_contents(&dir.path().join("out.docx"));
        assert_eq!(
            output.len(),
            3,
            "the failed media entry must not be dropped"
        );
        let png_entry = output
            .iter()
            .find(|(n, _)| n == "word/media/image1.png")
            .expect("failed media entry must exist");
        assert_eq!(
            png_entry.1, original_media,
            "failed media must keep its ORIGINAL bytes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn document_zero_media_docx_pure_rezip_or_pass_through() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("docProps/core.xml", b"<core>props</core>".to_vec()),
            ],
        );
        let input_size = fs::metadata(&input).unwrap().len();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                100,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        assert!(
            !result.met_target,
            "tiny target cannot be met without media — best effort"
        );
        assert!(
            result.strategy_used.contains("best-effort"),
            "strategy: {}",
            result.strategy_used
        );
        assert!(
            read_q_log(&dir.path()).is_empty(),
            "zero-media rezip must never invoke vips"
        );

        let output = read_zip_contents(&dir.path().join("out.docx"));
        assert_eq!(
            output.len(),
            3,
            "zero-media rezip must preserve every entry"
        );
        assert_eq!(output[0].0, "[Content_Types].xml");
        assert_eq!(output[0].1.as_slice(), CONTENT_TYPES);
        assert_eq!(output[2].1, b"<core>props</core>".to_vec());
        let out_size = fs::metadata(dir.path().join("out.docx")).unwrap().len();
        assert!(
            out_size <= input_size * 2,
            "pure rezip output {out_size} must stay in the ballpark of input {input_size}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn document_corrupt_zip_fails_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        let garbage = vec![b'x'; 4000];
        fs::write(&input, &garbage).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let result = DocumentProcessor.compress_to_target(
            &input,
            &dir.path().join("out.docx"),
            1024,
            &floors_with(40),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(fake)),
            &tmp,
        );

        match result {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("File may be corrupt"),
                    "corrupt zip must fail with the spec wording, got: {msg}"
                );
            }
            Ok(r) => panic!("corrupt zip must fail, got {}", r.strategy_used),
        }
        assert_eq!(
            fs::read(&input).unwrap(),
            garbage,
            "original file must be preserved"
        );
        assert!(
            !dir.path().join("out.docx").exists(),
            "no output may be produced for corrupt input"
        );
    }

    #[cfg(unix)]
    #[test]
    fn document_already_under_target() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        let original = vec![0xAB; 500];
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/media/image1.jpg", original.clone()),
            ],
        );
        let input_bytes = fs::read(&input).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                200 * 1024,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        assert_eq!(result.strategy_used, "already-under-target");
        assert!(result.met_target);
        assert_eq!(
            fs::read(&dir.path().join("out.docx")).unwrap(),
            input_bytes,
            "already-under-target must copy the file as-is, no re-zip churn"
        );
        assert!(
            read_q_log(&dir.path()).is_empty(),
            "already-under-target must never invoke vips"
        );
    }

    /// Waits until the fake vips q.log records at least `count` entries,
    /// then returns — a deterministic cancel trigger keyed to observed
    /// compressor progress, never wall-clock sleeps.
    #[cfg(unix)]
    fn wait_for_q_entries(dir: &Path, count: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if read_q_log(dir).len() >= count {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "q.log never reached {count} entries in time"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    #[cfg(unix)]
    fn document_cancel_before_delivery_no_output_no_temp_litter() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/a.jpg", noise_bytes(2000)),
                ("word/media/b.jpg", noise_bytes(2000)),
            ],
        );
        let input_bytes = fs::read(&input).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let token = CancellationToken::new();
        let worker_token = token.clone();
        let worker_input = input.clone();
        let worker_output = dir.path().join("out.docx");
        let worker_tmp = tmp.clone();
        let worker_fake = fake.clone();

        let handle = std::thread::spawn(move || {
            DocumentProcessor.compress_to_target(
                &worker_input,
                &worker_output,
                100,
                &floors_with(40),
                worker_token,
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(worker_fake)),
                &worker_tmp,
            )
        });

        // Deterministic trigger: cancel the instant the first vips
        // invocation is logged. With the fix, every delivery window
        // re-checks the token — the run completes no pass and never
        // delivers. Without the re-checks, the in-flight search can walk
        // to a fitting pass (100-byte media outputs at absurd q) and
        // deliver Done after cancel.
        wait_for_q_entries(&dir.path(), 1);
        token.cancel();
        let result = handle.join().unwrap();

        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "cancel before delivery must surface Cancelled, got {:?}",
            result.map(|r| (r.strategy_used, r.compressed_size))
        );
        assert!(
            !dir.path().join("out.docx").exists(),
            "no Done delivery after cancel: output file must not exist"
        );
        assert_eq!(
            fs::read(&input).unwrap(),
            input_bytes,
            "input file must be byte-identical after cancel"
        );
        let remaining: Vec<_> = fs::read_dir(&tmp).unwrap().flatten().collect();
        assert!(
            remaining.is_empty(),
            "temp entries must be cleaned after cancel, found: {:?}",
            remaining.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }

    #[test]
    #[cfg(unix)]
    fn document_cancel_mid_recompress_cleans_temp_entries() {
        let dir = tempfile::tempdir().unwrap();
        // Second media (b.jpg, index 3) sleeps 5s inside vips so the cancel
        // lands inside a live sidecar spawn deterministically.
        let fake = make_executable_script(
            dir.path(),
            "fake-vips-sleep",
            r#"#!/bin/bash
out="$3"
base="${out%%\[*}"
q="${out#*\[Q=}"; q="${q%,*}"
echo "$q" >> "$(dirname "$0")/q.log"
case "$base" in
  *media-0-3*) sleep 5 ;;
esac
n=$(( q * 100 ))
awk -v n="$n" 'BEGIN { for (i = 0; i < n; i++) printf "%c", i % 251 }' > "$base"
"#,
        );
        let input = dir.path().join("in.docx");
        let input_bytes = noise_bytes(8400);
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/a.jpg", noise_bytes(8000)),
                ("word/media/b.jpg", noise_bytes(8000)),
            ],
        );
        let input_bytes = fs::read(&input).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let token = CancellationToken::new();
        let worker_token = token.clone();
        let worker_input = input.clone();
        let worker_output = dir.path().join("out.docx");
        let worker_tmp = tmp.clone();
        let worker_fake = fake.clone();

        let handle = std::thread::spawn(move || {
            DocumentProcessor.compress_to_target(
                &worker_input,
                &worker_output,
                5 * 1024,
                &floors_with(40),
                worker_token,
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(worker_fake)),
                &worker_tmp,
            )
        });
        // Deterministic trigger: both medias of pass 0 have entered vips
        // (a.jpg finished, b.jpg sleeping) — cancel lands inside the live
        // sidecar spawn, never wall-clock tuned.
        wait_for_q_entries(&dir.path(), 2);
        token.cancel();
        let result = handle.join().unwrap();

        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "cancel mid-recompress must surface as Cancelled, got {:?}",
            result.map(|r| (r.strategy_used, r.compressed_size))
        );
        assert!(
            !dir.path().join("out.docx").exists(),
            "no Done delivery after cancel"
        );
        assert_eq!(
            fs::read(&input).unwrap(),
            input_bytes,
            "input file must be byte-identical after mid-recompress cancel"
        );
        let remaining: Vec<_> = fs::read_dir(&tmp).unwrap().flatten().collect();
        assert!(
            remaining.is_empty(),
            "temp entries must be cleaned after cancel, found: {:?}",
            remaining.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }

    /// Waits until `name` appears in `dir` — deterministic trigger keyed to
    /// observed worker progress, never wall-clock sleeps.
    fn wait_for_file(dir: &Path, name: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if dir.join(name).exists() {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{name} never appeared in {} in time",
                dir.display()
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    #[cfg(unix)]
    fn document_cancel_during_rezip_preserves_preexisting_output_file() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        // Many entries so the rezip takes long enough for the cancel to
        // land inside it: attempt-0.docx is created the instant the rezip
        // starts, long before delivery.
        let mut entries: Vec<(String, Vec<u8>)> =
            vec![("[Content_Types].xml".to_string(), CONTENT_TYPES.to_vec())];
        for i in 0..1200 {
            entries.push((format!("part{i}.xml"), varied_text(40)));
        }
        let entries: Vec<(&str, Vec<u8>)> = entries
            .iter()
            .map(|(n, c)| (n.as_str(), c.clone()))
            .collect();
        let input = dir.path().join("in.docx");
        build_docx(&input, &entries);
        let input_size = fs::metadata(&input).unwrap().len();
        assert!(
            input_size > 100 * 1024,
            "input {input_size} must exceed the target so the rezip path runs"
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // A pre-existing file at the exact output path: an external race
        // appeared under the resolved name between resolution and delivery.
        // The exists-snapshot guard must treat it as foreign and NEVER
        // delete it on late cancel.
        let output_path = dir.path().join("out.docx");
        fs::write(&output_path, b"precious-unrelated-content").unwrap();

        let token = CancellationToken::new();
        let worker_token = token.clone();
        let (worker_input, worker_output, worker_tmp, worker_fake) =
            (input.clone(), output_path.clone(), tmp.clone(), fake.clone());

        let handle = std::thread::spawn(move || {
            DocumentProcessor.compress_to_target(
                &worker_input,
                &worker_output,
                24 * 1024,
                &floors_with(40),
                worker_token,
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(worker_fake)),
                &worker_tmp,
            )
        });

        wait_for_file(&tmp, "attempt-0.docx");
        token.cancel();
        let result = handle.join().unwrap();

        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "cancel during rezip must surface Cancelled, got {:?}",
            result.map(|r| (r.strategy_used, r.compressed_size))
        );
        assert_eq!(
            fs::read(&output_path).unwrap(),
            b"precious-unrelated-content",
            "a file occupied at the output path before our copy must never be deleted on cancel"
        );
        assert!(
            !dir.path().join("out.docx.collided").exists(),
            "collided delivery must not land (cancel wins over copy)"
        );
        let remaining: Vec<_> = fs::read_dir(&tmp).unwrap().flatten().collect();
        assert!(
            remaining.is_empty(),
            "temp entries must be cleaned after cancel, found: {:?}",
            remaining.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
        assert_eq!(
            fs::read(&input).unwrap().len(),
            input_size as usize,
            "input file must be untouched"
        );
    }

    #[test]
    #[cfg(unix)]
    fn document_cancel_during_zero_media_rezip_no_output_no_temp() {
        // Same harness, uncontended output path: cancel during the zero-media
        // rezip must surface Cancelled. (Naming kept for the temp-litter
        // guarantee; the pre-existing-output case is the test above.)
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let mut entries: Vec<(String, Vec<u8>)> =
            vec![("[Content_Types].xml".to_string(), CONTENT_TYPES.to_vec())];
        for i in 0..1200 {
            entries.push((format!("part{i}.xml"), varied_text(40)));
        }
        let entries: Vec<(&str, Vec<u8>)> = entries
            .iter()
            .map(|(n, c)| (n.as_str(), c.clone()))
            .collect();
        let input = dir.path().join("in.docx");
        build_docx(&input, &entries);
        let input_size = fs::metadata(&input).unwrap().len();
        assert!(
            input_size > 24 * 1024,
            "input {input_size} must exceed the target so the rezip path runs"
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let token = CancellationToken::new();
        let worker_token = token.clone();
        let (worker_input, worker_output, worker_tmp, worker_fake) =
            (input.clone(), dir.path().join("out.docx"), tmp.clone(), fake.clone());

        let handle = std::thread::spawn(move || {
            DocumentProcessor.compress_to_target(
                &worker_input,
                &worker_output,
                24 * 1024,
                &floors_with(40),
                worker_token,
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(worker_fake)),
                &worker_tmp,
            )
        });

        // Deterministic: cancel the instant attempt-0.docx appears — that
        // is the first statement of the rezip, with delivery minutes of
        // deflate work still ahead. The deliver window must re-check the
        // token; without that check the rezip finishes and Done is
        // delivered despite the cancel.
        wait_for_file(&tmp, "attempt-0.docx");
        token.cancel();
        let result = handle.join().unwrap();

        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "cancel during zero-media rezip must surface Cancelled, got {:?}",
            result.map(|r| (r.strategy_used, r.compressed_size))
        );
        assert!(
            !dir.path().join("out.docx").exists(),
            "no Done delivery after cancel: output file must not exist"
        );
        let remaining: Vec<_> = fs::read_dir(&tmp).unwrap().flatten().collect();
        assert!(
            remaining.is_empty(),
            "temp entries must be cleaned after cancel, found: {:?}",
            remaining.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }

    #[cfg(unix)]
    #[test]
    fn document_output_opens_valid() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/media/image1.jpg", noise_bytes(8000)),
                ("word/media/image2.png", vec![9, 9, 9]),
                ("word/media/image3.wmf", vec![1, 2, 3]),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                14 * 1024,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();
        assert!(result.met_target, "strategy: {}", result.strategy_used);

        let input_names = read_zip_contents(&input)
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>();
        let output_names = read_zip_contents(&dir.path().join("out.docx"))
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>();
        assert_eq!(
            output_names, input_names,
            "output must open as a valid zip with identical entry count and names"
        );
    }

    /// Fake vips with a deliberately non-monotonic size curve: q ≥ 65 →
    /// 9000 bytes; q ≤ 54 → 9000 bytes; only 55..=64 → 6000 bytes. With
    /// floor 50 the search hits 72, 60, 54, 51, 50 — first, last, and
    /// lowest-quality passes are all 9000, only the mid pass (q=60) is the
    /// smallest (6000).
    #[cfg(unix)]
    fn write_fake_vips_bump(dir: &Path) -> String {
        make_executable_script(
            dir,
            "fake-vips-bump",
            r#"#!/bin/bash
out="$3"
base="${out%%\[*}"
q="${out#*\[Q=}"; q="${q%,*}"; q="${q%%\]*}"
echo "$q" >> "$(dirname "$0")/q.log"
n=6000
if [ "$q" -ge 65 ]; then n=9000; fi
if [ "$q" -le 54 ]; then n=9000; fi
awk -v n="$n" 'BEGIN { s = 12345; for (i = 0; i < n; i++) { s = (s * 16807 + 11) % 2147483647; printf "%c", int(s / 32768) % 255 + 1 } }' > "$base"
"#,
        )
    }

    #[test]
    #[cfg(unix)]
    fn document_best_effort_delivers_smallest_over_target_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips_bump(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.jpg", noise_bytes(8000)),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // Target smaller than every possible attempt outcome; all passes
        // are best-effort candidates, none fits.
        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                100,
                &floors_with(50),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        assert!(
            !result.met_target,
            "unreachable target must be best-effort, strategy: {}",
            result.strategy_used
        );
        assert_eq!(
            result.strategy_used, "quality=60,best-effort",
            "the smallest pass (q=60) must be the delivered strategy"
        );

        let log = read_q_log(&dir.path());
        assert_eq!(
            *log.iter().min().unwrap(),
            50,
            "floor attempt must have run, got {log:?}"
        );
        assert!(
            log.iter().all(|q| *q >= 50),
            "no attempt may go below the floor 50, got {log:?}"
        );

        // First (q=72), last (q=50), and lowest-quality (q=50) passes all
        // produce 9000 bytes — only the mid q=60 pass is smallest. The
        // delivered media must be the smallest attempt's bytes.
        let output = read_zip_contents(&dir.path().join("out.docx"));
        let media_entry = output
            .iter()
            .find(|(n, _)| n == "word/media/image1.jpg")
            .expect("media entry must survive");
        assert_eq!(
            media_entry.1,
            noise_bytes(6000),
            "delivered media must be the SMALLEST attempt's bytes, not a later/bigger pass"
        );
        assert!(
            result.compressed_size <= 7000,
            "delivered size {} must track the smallest attempt, not a bigger one",
            result.compressed_size
        );
    }

    #[test]
    #[cfg(unix)]
    fn document_progress_monotonic_within_bounds_ends_at_100() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/a.jpg", noise_bytes(2000)),
                ("word/media/b.jpg", noise_bytes(2000)),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let (progress, seen) = ProgressSender::test_collector();

        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                6144,
                &floors_with(40),
                CancellationToken::new(),
                progress,
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        assert!(result.met_target, "strategy: {}", result.strategy_used);
        let events = seen.lock().unwrap().clone();
        assert!(!events.is_empty(), "progress events must exist");
        for pair in events.windows(2) {
            assert!(
                pair[0] <= pair[1],
                "progress must be monotonically non-decreasing: {events:?}"
            );
        }
        let pre_delivery = &events[..events.len() - 1];
        assert!(
            pre_delivery.iter().all(|&p| p <= 90),
            "pre-delivery events must stay <= 90: {events:?}"
        );
        assert_eq!(
            *events.last().unwrap(),
            100,
            "delivery must emit terminal 100: {events:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn document_vips_exit0_empty_output_keeps_original() {
        let dir = tempfile::tempdir().unwrap();
        let fake = make_executable_script(
            dir.path(),
            "fake-vips-empty",
            r#"#!/bin/bash
out="$3"
base="${out%%\[*}"
: > "$base"
"#,
        );
        let original = noise_bytes(8000);
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.png", original.clone()),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                9 * 1024,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        let output = read_zip_contents(&dir.path().join("out.docx"));
        let png_entry = output
            .iter()
            .find(|(n, _)| n == "word/media/image1.png")
            .expect("media entry must survive");
        assert_eq!(
            png_entry.1, original,
            "exit-0 empty vips output must keep the ORIGINAL media bytes"
        );
    }

    #[test]
    #[cfg(unix)]
    fn document_vips_exit0_unreadable_output_keeps_original() {
        let dir = tempfile::tempdir().unwrap();
        let fake = make_executable_script(
            dir.path(),
            "fake-vips-vanish",
            r#"#!/bin/bash
out="$3"
base="${out%%\[*}"
printf 'x' > "$base"
rm "$base"
"#,
        );
        let original = noise_bytes(8000);
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.png", original.clone()),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                9 * 1024,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        let output = read_zip_contents(&dir.path().join("out.docx"));
        let png_entry = output
            .iter()
            .find(|(n, _)| n == "word/media/image1.png")
            .expect("media entry must survive");
        assert_eq!(
            png_entry.1, original,
            "unreadable (deleted) vips output must keep the ORIGINAL media bytes"
        );
    }

    #[test]
    #[cfg(unix)]
    fn document_sidecar_spawn_failure_keeps_original_media_bytes() {
        let dir = tempfile::tempdir().unwrap();
        // Non-executable file at the sidecar path — spawn fails outright.
        let sidecar = dir.path().join("not-executable-vips");
        fs::write(&sidecar, b"definitely not a program").unwrap();
        let sidecar = sidecar.to_string_lossy().to_string();
        let original = noise_bytes(8000);
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.png", original.clone()),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                9 * 1024,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(sidecar)),
                &tmp,
            )
            .unwrap();

        let output = read_zip_contents(&dir.path().join("out.docx"));
        let png_entry = output
            .iter()
            .find(|(n, _)| n == "word/media/image1.png")
            .expect("media entry must survive");
        assert_eq!(
            png_entry.1, original,
            "spawn failure must keep the ORIGINAL media bytes and continue"
        );
    }

    #[cfg(unix)]
    #[test]
    fn document_rezip_duplicate_entry_names_fail_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_raw_stored_zip(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/media/image1.jpg", noise_bytes(8000)),
                ("word/media/image1.jpg", b"second copy".to_vec()),
            ],
        );
        let input_bytes = fs::read(&input).unwrap();
        let input_size = input_bytes.len() as u64;
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // The zip crate's writer rejects duplicate filenames, so a
        // duplicate-entry archive can never round-trip once recompression
        // is attempted. Whatever stage rejects it, the guarantee is: no
        // crash, no delivery, and the original file preserved. The target
        // sits below the input size so the already-under-target copy path
        // cannot silently absorb the case.
        let result = DocumentProcessor.compress_to_target(
            &input,
            &dir.path().join("out.docx"),
            input_size - 486,
            &floors_with(40),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(fake)),
            &tmp,
        );

        assert!(
            result.is_err(),
            "duplicate-entry zip must not deliver, got {:?}",
            result.map(|r| (r.strategy_used, r.compressed_size))
        );
        assert!(
            !dir.path().join("out.docx").exists(),
            "no output may be produced for duplicate-entry input"
        );
        assert_eq!(
            fs::read(&input).unwrap(),
            input_bytes,
            "original file must be preserved"
        );
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for i in 0..256u32 {
            let mut c = i;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            table[i as usize] = c;
        }
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc = table[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
        }
        crc ^ 0xFFFF_FFFF
    }

    /// Hand-rolled stored-method zip — for archives the ZipWriter refuses
    /// to create (duplicate entry names).
    fn build_raw_stored_zip(path: &Path, entries: &[(&str, Vec<u8>)]) {
        let mut buf: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, content) in entries {
            let offset = buf.len() as u32;
            let crc = crc32(content);
            buf.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
            buf.extend_from_slice(&20u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&crc.to_le_bytes());
            buf.extend_from_slice(&(content.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(content.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(name.as_bytes());
            buf.extend_from_slice(content);

            central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(content.len() as u32).to_le_bytes());
            central.extend_from_slice(&(content.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let central_offset = buf.len() as u32;
        buf.extend_from_slice(&central);
        buf.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        buf.extend_from_slice(&(central.len() as u32).to_le_bytes());
        buf.extend_from_slice(&central_offset.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        fs::write(path, &buf).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn document_empty_archive_rezip_produces_valid_eocd() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(&input, &[]);
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        // Tiny target (below the 22-byte empty zip) so the already-under-target
        // early exit cannot route this to the copy path — it must hit the
        // real zero-media rezip. The rezip of an empty archive stays 22 bytes,
        // over-target → deterministic best-effort strategy pin proving the
        // routing (a copy path would deliver "already-under-target").
        let input_size = fs::metadata(&input).unwrap().len();
        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                input_size.saturating_sub(1),
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        assert_eq!(
            result.strategy_used, "rezip-deflate9,best-effort",
            "empty archive must exercise the REZIP path, not the under-target copy"
        );
        assert!(
            !result.met_target,
            "21-byte target cannot fit the 22-byte rezipped empty archive"
        );
        assert_eq!(
            result.compressed_size, input_size,
            "empty archive rezipped at level 9 must stay byte-identical in size"
        );
        let out_path = dir.path().join("out.docx");
        assert_eq!(fs::metadata(&out_path).unwrap().len(), input_size);
        assert!(
            zip::ZipArchive::new(fs::File::open(&out_path).unwrap()).is_ok(),
            "round-trip must still be a readable zip (valid EOCD, no crash)"
        );
    }

    /// Real-vips end-to-end smoke (plan Task 2 Step 3): builds a DOCX with
    /// a large noise JPEG embedded, compresses with the real sidecar, and
    /// verifies the output is a valid zip whose media shrank. Ignored —
    /// requires the real vips sidecar on this machine.
    #[test]
    #[cfg(unix)]
    #[ignore = "requires real vips sidecar on this machine (Task 2 Step 3 real smoke)"]
    fn real_vips_compresses_docx_media_to_target() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = format!(
            "{}/binaries/{}",
            env!("CARGO_MANIFEST_DIR"),
            crate::sidecar::sidecar_filename("vips").unwrap()
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let noise = dir.path().join("noise.jpg");
        let out = std::process::Command::new(&sidecar)
            .args(["gaussnoise"])
            .arg(&noise)
            .args(["1500", "1500"])
            .output()
            .expect("gaussnoise generation");
        assert!(out.status.success(), "gaussnoise failed: {:?}", out.stderr);
        let noise_bytes = fs::read(&noise).unwrap();
        assert!(
            noise_bytes.len() > 100 * 1024,
            "noise image unexpectedly small: {} bytes",
            noise_bytes.len()
        );

        let input = dir.path().join("report.docx");
        let docx_file = fs::File::create(&input).unwrap();
        let mut writer = ZipWriter::new(docx_file);
        writer
            .start_file(
                "[Content_Types].xml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#,
            )
            .unwrap();
        writer
            .start_file(
                "word/document.xml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer
            .write_all(br#"<?xml version="1.0"?><w:document xmlns:w="x"/>"#)
            .unwrap();
        writer
            .start_file(
                "word/media/image1.jpg",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(&noise_bytes).unwrap();
        writer.finish().unwrap();

        let input_size = fs::metadata(&input).unwrap().len();
        assert!(input_size > 50 * 1024, "input unexpectedly small: {input_size}");

        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                1 * 1024 * 1024,
                &crate::settings::QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(sidecar)),
                &tmp,
            )
            .unwrap();

        assert!(result.compressed_size < input_size, "strategy: {}", result.strategy_used);
        let output = read_zip_contents(&dir.path().join("out.docx"));
        assert_eq!(output.len(), 3, "entry count must be preserved");
        assert_eq!(output[0].0, "[Content_Types].xml");
        let recompressed = &output[2].1;
        assert!(
            recompressed.len() < noise_bytes.len(),
            "media must shrink: {} -> {}",
            noise_bytes.len(),
            recompressed.len()
        );
    }
}
