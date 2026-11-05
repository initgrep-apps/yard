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

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[cfg(test)]
thread_local! {
    static OPEN_ENTRIES: std::cell::RefCell<usize> = std::cell::RefCell::new(0);
    static MAX_OPEN_ENTRIES: std::cell::RefCell<usize> = std::cell::RefCell::new(0);
    static FINISH_COUNT: std::cell::RefCell<usize> = std::cell::RefCell::new(0);
}

use tokio_util::sync::CancellationToken;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::engine::result::STRATEGY_ALREADY_UNDER_TARGET;
use crate::engine::{
    CompressionError, CompressionProcessor, CompressionResult, ProgressSender, SidecarName,
    SidecarPaths, deliver_output, run_sidecar, strict_exit,
};
use crate::models::Category;
use crate::settings::QualityFloors;

pub struct DocumentProcessor;

const MAX_SEARCH_ITERATIONS: u32 = 8;
const START_QUALITY: u32 = 95;
const DEFLATE_LEVEL: i64 = 9;
const CENTRAL_DIR_MARGIN: u64 = 1024;

pub const MAX_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_COMPRESSION_RATIO: u64 = 200;

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

/// Metadata for one zip entry, read from the central directory before any
/// decompression.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EntryMeta {
    pub index: usize,
    pub name: String,
    pub is_dir: bool,
    pub compression: zip::CompressionMethod,
    pub size: u64,
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
        "png" => "[compression=9,palette,colours=256,keep=icc]".to_string(),
        _ => format!("[Q={},keep=icc]", quality),
    }
}

fn media_ext(entry_name: &str) -> String {
    std::path::Path::new(entry_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default()
}

fn deflate_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(DEFLATE_LEVEL))
}

fn stored_options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
}

const CORRUPT: &str = "File may be corrupt (not a readable OOXML zip container)";

fn input_file_error(input: &Path, e: impl std::fmt::Display) -> CompressionError {
    CompressionError::InputFileError(format!("{}: {e}", input.display()))
}

fn output_file_error(path: &Path, e: impl std::fmt::Display) -> CompressionError {
    CompressionError::OutputFileError(format!("{}: {e}", path.display()))
}

/// Reads the central-directory metadata for every entry and rejects archives
/// whose declared uncompressed total or compression ratio exceeds the safety
/// limits before any bytes are decompressed.
pub fn read_index(input: &Path) -> Result<Vec<EntryMeta>, CompressionError> {
    let file = std::fs::File::open(input)
        .map_err(|e| input_file_error(input, e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        eprintln!("warn: cannot read {} as zip container: {e}", input.display());
        CompressionError::SidecarFailed(CORRUPT.to_string())
    })?;
    let mut metas = Vec::with_capacity(archive.len());
    let mut total_uncompressed: u64 = 0;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|e| {
            eprintln!("warn: cannot read entry {index} of {}: {e}", input.display());
            CompressionError::SidecarFailed(CORRUPT.to_string())
        })?;
        let name = entry.enclosed_name().ok_or_else(|| {
            eprintln!("warn: entry {index} of {} has an unsafe path", input.display());
            CompressionError::SidecarFailed(CORRUPT.to_string())
        })?;
        let name = name.to_string_lossy().into_owned();
        let is_dir = entry.is_dir();
        let compression = entry.compression();
        let size = entry.size();
        total_uncompressed = total_uncompressed.saturating_add(size);
        metas.push(EntryMeta {
            index,
            name,
            is_dir,
            compression,
            size,
        });
    }
    let mut seen_names = std::collections::HashSet::new();
    for meta in &metas {
        if !seen_names.insert(meta.name.clone()) {
            return Err(CompressionError::InputFileError(
                "Duplicate entry names in archive".to_string(),
            ));
        }
    }

    if total_uncompressed > MAX_UNCOMPRESSED_BYTES {
        return Err(CompressionError::InputFileError(
            "Archive declared uncompressed total exceeds 4 GiB".to_string(),
        ));
    }
    let compressed_size = std::fs::metadata(input)
        .map_err(|e| input_file_error(input, e))?
        .len()
        .max(1);
    if total_uncompressed.saturating_div(compressed_size) > MAX_COMPRESSION_RATIO {
        return Err(CompressionError::InputFileError(
            "Archive compression ratio exceeds 200:1".to_string(),
        ));
    }
    Ok(metas)
}

/// Extracts every media entry (JPEG/PNG/EMF/WMF/…) to a temp file named
/// `media-{index}.{ext}`. Only one `ZipFile` is open at a time.
pub fn extract_media(
    input: &Path,
    metas: &[EntryMeta],
    temp: &Path,
) -> Result<HashMap<usize, PathBuf>, CompressionError> {
    let file = std::fs::File::open(input)
        .map_err(|e| input_file_error(input, e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        eprintln!("warn: cannot read {} as zip container: {e}", input.display());
        CompressionError::SidecarFailed(CORRUPT.to_string())
    })?;
    let mut media = HashMap::new();
    for meta in metas {
        if meta.is_dir || media_disposition(&meta.name) == MediaDisposition::NotMedia {
            continue;
        }
        let mut entry = archive.by_index(meta.index).map_err(|e| {
            eprintln!("warn: cannot read zip entry {} of {}: {e}", meta.name, input.display());
            CompressionError::SidecarFailed(CORRUPT.to_string())
        })?;
        #[cfg(test)]
        OPEN_ENTRIES.with(|c| {
            let mut n = c.borrow_mut();
            *n += 1;
            MAX_OPEN_ENTRIES.with(|m| {
                let mut m = m.borrow_mut();
                *m = (*m).max(*n);
            });
        });
        let ext = media_ext(&meta.name);
        let path = temp.join(format!("media-{}.{}", meta.index, ext));
        let mut out = std::fs::File::create(&path)
            .map_err(|e| output_file_error(&path, e))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| output_file_error(&path, e))?;
        drop(entry);
        #[cfg(test)]
        OPEN_ENTRIES.with(|c| *c.borrow_mut() -= 1);
        media.insert(meta.index, path);
    }
    Ok(media)
}

/// Writer adapter that tracks the logical write position of the underlying
/// writer. Writes advance the position; seeks set it. This matches the final
/// file size even when `ZipWriter` patches headers by seeking backward.
struct CountingWriter<W: Write> {
    inner: W,
    position: u64,
}

impl<W: Write> CountingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            position: 0,
        }
    }

    fn position(&self) -> u64 {
        self.position
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.position += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl<W: Write + Seek> Seek for CountingWriter<W> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let new_pos = self.inner.seek(pos)?;
        self.position = new_pos;
        Ok(new_pos)
    }
}

/// Measures the size of the fixed (non-media) part of the archive by streaming
/// every non-media entry through a `ZipWriter` backed by a `CountingWriter`.
/// The returned `fixed_len` is the number of bytes the writer emitted, which
/// the quality search uses together with the media-file sizes.
fn measure_fixed_part(
    input: &Path,
    metas: &[EntryMeta],
    temp: &Path,
) -> Result<u64, CompressionError> {
    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("docx");
    let scratch = temp.join(format!("fixed.{}", ext));
    let file = std::fs::File::create(&scratch)
        .map_err(|e| output_file_error(&scratch, e))?;
    let counter = CountingWriter::new(file);
    let mut writer = zip::ZipWriter::new(counter);
    let input_file = std::fs::File::open(input)
        .map_err(|e| input_file_error(input, e))?;
    let mut archive = zip::ZipArchive::new(input_file).map_err(|e| {
        eprintln!("warn: cannot read {} as zip container: {e}", input.display());
        CompressionError::SidecarFailed(CORRUPT.to_string())
    })?;
    for meta in metas {
        if meta.is_dir {
            writer
                .add_directory(meta.name.clone(), deflate_options())
                .map_err(|e| CompressionError::OutputFileError(format!("rezip: {e}")))?;
            continue;
        }
        if media_disposition(&meta.name) != MediaDisposition::NotMedia {
            continue;
        }
        let mut entry = archive.by_index(meta.index).map_err(|e| {
            eprintln!("warn: cannot read zip entry {} of {}: {e}", meta.name, input.display());
            CompressionError::SidecarFailed(CORRUPT.to_string())
        })?;
        writer
            .start_file(meta.name.clone(), deflate_options())
            .map_err(|e| CompressionError::OutputFileError(format!("rezip: {e}")))?;
        std::io::copy(&mut entry, &mut writer)
            .map_err(|e| CompressionError::OutputFileError(format!("rezip entry {}: {e}", meta.name)))?;
    }
    let counter = writer
        .finish()
        .map_err(|e| CompressionError::OutputFileError(format!("rezip finish: {e}")))?;
    let fixed_len = counter.position();
    let scratch_len = std::fs::metadata(&scratch)
        .map_err(|e| output_file_error(&scratch, e))?
        .len();
    debug_assert_eq!(
        fixed_len, scratch_len,
        "CountingWriter position must match the final scratch file size"
    );
    let _ = std::fs::remove_file(&scratch);
    Ok(fixed_len)
}

/// Writes a complete archive: non-media entries streamed from `input`; media
/// entries are streamed from `media` and stored uncompressed. The chosen media
/// file is compared against the original entry size, and if it grew, the
/// original entry bytes are streamed instead. The file is finished but not
/// committed to the final output path.
pub fn write_archive(
    input: &Path,
    metas: &[EntryMeta],
    media: &HashMap<usize, PathBuf>,
    out: &Path,
) -> Result<(), CompressionError> {
    let file = std::fs::File::create(out)
        .map_err(|e| output_file_error(out, e))?;
    let mut writer = zip::ZipWriter::new(file);
    let input_file = std::fs::File::open(input)
        .map_err(|e| input_file_error(input, e))?;
    let mut archive = zip::ZipArchive::new(input_file).map_err(|e| {
        eprintln!("warn: cannot read {} as zip container: {e}", input.display());
        CompressionError::SidecarFailed(CORRUPT.to_string())
    })?;
    for meta in metas {
        if meta.is_dir {
            writer
                .add_directory(meta.name.clone(), deflate_options())
                .map_err(|e| CompressionError::OutputFileError(format!("rezip: {e}")))?;
            continue;
        }
        if media_disposition(&meta.name) == MediaDisposition::NotMedia {
            let mut entry = archive.by_index(meta.index).map_err(|e| {
                eprintln!("warn: cannot read zip entry {} of {}: {e}", meta.name, input.display());
                CompressionError::SidecarFailed(CORRUPT.to_string())
            })?;
            writer
                .start_file(meta.name.clone(), deflate_options())
                .map_err(|e| CompressionError::OutputFileError(format!("rezip: {e}")))?;
            std::io::copy(&mut entry, &mut writer)
                .map_err(|e| CompressionError::OutputFileError(format!("rezip entry {}: {e}", meta.name)))?;
            continue;
        }

        let chosen_path = media.get(&meta.index).ok_or_else(|| {
            CompressionError::OutputFileError(format!("missing media for {}", meta.name))
        })?;
        let original_len = archive.by_index(meta.index)
            .map_err(|e| {
                eprintln!("warn: cannot read zip entry {} of {}: {e}", meta.name, input.display());
                CompressionError::SidecarFailed(CORRUPT.to_string())
            })?
            .size();
        let chosen_len = std::fs::metadata(chosen_path)
            .map_err(|e| output_file_error(chosen_path, e))?
            .len();

        let use_original = chosen_len > original_len;
        let method = if use_original { deflate_options() } else { stored_options() };
        writer
            .start_file(meta.name.clone(), method)
            .map_err(|e| CompressionError::OutputFileError(format!("rezip: {e}")))?;

        if use_original {
            let mut entry = archive.by_index(meta.index).map_err(|e| {
                eprintln!("warn: cannot read zip entry {} of {}: {e}", meta.name, input.display());
                CompressionError::SidecarFailed(CORRUPT.to_string())
            })?;
            std::io::copy(&mut entry, &mut writer)
                .map_err(|e| CompressionError::OutputFileError(format!("rezip entry {}: {e}", meta.name)))?;
        } else {
            let mut src = std::fs::File::open(chosen_path)
                .map_err(|e| output_file_error(chosen_path, e))?;
            std::io::copy(&mut src, &mut writer)
                .map_err(|e| CompressionError::OutputFileError(format!("rezip entry {}: {e}", meta.name)))?;
        }
    }
    writer
        .finish()
        .map_err(|e| CompressionError::OutputFileError(format!("rezip finish: {e}")))?;
    #[cfg(test)]
    FINISH_COUNT.with(|c| *c.borrow_mut() += 1);
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
    let result = deliver_output(
        &path, output, size, strategy, quality, met_target, progress, cancel_token,
    );
    if result.is_err() {
        remove_all(temp_paths);
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
    attempt: &mut dyn FnMut(u32, u32) -> Result<(u64, HashMap<usize, PathBuf>), CompressionError>,
) -> Result<
    (
        Option<(u32, u64, HashMap<usize, PathBuf>)>,
        Option<(u32, u64, HashMap<usize, PathBuf>)>,
    ),
    CompressionError,
> {
    debug_assert!(lo <= hi, "search_quality requires lo <= hi");
    let mut lo = lo;
    let mut hi = hi;
    let mut best_fit: Option<(u32, u64, HashMap<usize, PathBuf>)> = None;
    let mut best_effort: Option<(u32, u64, HashMap<usize, PathBuf>)> = None;
    let mut pass: u32 = 0;

    for _iteration in 0..max_iterations {
        if cancel_token.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }
        if lo > hi {
            break;
        }
        let mid = (lo + hi) / 2;
        let (size, chosen) = attempt(pass, mid)?;
        pass += 1;
        if size <= target_bytes {
            best_fit = Some((mid, size, chosen));
            lo = mid + 1;
        } else {
            if best_effort.as_ref().map(|(_, s, _)| size < *s).unwrap_or(true) {
                best_effort = Some((mid, size, chosen));
            }
            hi = mid.saturating_sub(1);
        }
    }
    Ok((best_fit, best_effort))
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
        let floor = quality_floor.document.min_media_quality;
        let (min_quality, max_quality) = if floor > START_QUALITY {
            (floor, floor)
        } else {
            (floor, START_QUALITY)
        };
        let ext = input
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_else(|| "docx".to_string());

        let metas = read_index(input)?;
        let media_total = metas
            .iter()
            .filter(|m| media_disposition(&m.name) == MediaDisposition::Recompress)
            .count();
        let temp_paths = Rc::new(RefCell::new(Vec::<PathBuf>::new()));

        if cancel_token.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }

        let media_map = extract_media(input, &metas, temp_dir)?;
        {
            let mut paths = temp_paths.borrow_mut();
            for path in media_map.values() {
                paths.push(path.clone());
            }
        }

        let fixed_len = measure_fixed_part(input, &metas, temp_dir)?;

        if media_total == 0 {
            let attempt_path = temp_dir.join(format!("attempt-0.{}", ext));
            temp_paths.borrow_mut().push(attempt_path.clone());
            write_archive(input, &metas, &media_map, &attempt_path)?;
            let size = std::fs::metadata(&attempt_path)
                .map_err(|e| output_file_error(&attempt_path, e))?
                .len();
            let pending = Ok((
                attempt_path,
                size,
                if size <= target_bytes {
                    "rezip-deflate9".to_string()
                } else {
                    "rezip-deflate9,best-effort".to_string()
                },
                100,
                size <= target_bytes,
            ));
            return deliver_or_cancel(
                pending,
                output,
                &cancel_token,
                &*temp_paths.borrow(),
                &progress,
            );
        }

        let temp_paths_for_attempt = Rc::clone(&temp_paths);
        let mut attempt = |pass: u32, quality: u32| -> Result<(u64, HashMap<usize, PathBuf>), CompressionError> {
            if cancel_token.is_cancelled() {
                return Err(CompressionError::Cancelled);
            }
            let mut chosen: HashMap<usize, PathBuf> = HashMap::new();
            let mut media_done: usize = 0;
            let mut media_sum: u64 = 0;

            for meta in metas.iter() {
                let disposition = media_disposition(&meta.name);
                if disposition == MediaDisposition::NotMedia || meta.is_dir {
                    continue;
                }
                progress.send_progress(per_media_progress(pass, media_done, media_total));
                media_done += 1;

                let original = media_map
                    .get(&meta.index)
                    .cloned()
                    .ok_or_else(|| CompressionError::OutputFileError(format!("missing media for {}", meta.name)))?;
                let ext = media_ext(&meta.name);
                let mut picked = original.clone();

                if disposition == MediaDisposition::Recompress {
                    let recompressed = temp_dir.join(format!("media-{}-q{}.{}", meta.index, quality, ext));
                    temp_paths_for_attempt.borrow_mut().push(recompressed.clone());
                    let args = vec![
                        "copy".to_string(),
                        original.display().to_string(),
                        format!("{}{}", recompressed.display(), media_save_options(&ext, quality)),
                    ];
                    match run_sidecar(sidecar, "vips", &args, &cancel_token, strict_exit) {
                        Ok(()) => match std::fs::metadata(&recompressed) {
                            Ok(m) if m.len() > 0 => {
                                let original_len = std::fs::metadata(&original).map(|m| m.len()).unwrap_or(u64::MAX);
                                if m.len() < original_len {
                                    picked = recompressed.clone();
                                }
                            }
                            Ok(_) => {
                                eprintln!("warn: vips produced empty output for {}, keeping original", meta.name);
                            }
                            Err(e) => {
                                eprintln!("warn: cannot read recompressed media {}: {e}, keeping original", meta.name);
                            }
                        },
                        Err(CompressionError::Cancelled) => return Err(CompressionError::Cancelled),
                        Err(e) => {
                            eprintln!("warn: vips failed for {}, keeping original bytes: {e}", meta.name);
                        }
                    }
                }

                let picked_len = std::fs::metadata(&picked)
                    .map_err(|e| output_file_error(&picked, e))?
                    .len();
                chosen.insert(meta.index, picked);
                media_sum += picked_len;
            }

            let predicted = fixed_len + media_sum + CENTRAL_DIR_MARGIN;
            Ok((predicted, chosen))
        };

        let search = search_quality(
            min_quality,
            max_quality,
            MAX_SEARCH_ITERATIONS,
            target_bytes,
            &cancel_token,
            &mut attempt,
        );

        let temp_paths_for_build = Rc::clone(&temp_paths);
        let build_and_deliver =
            |quality: u32,
             chosen: HashMap<usize, PathBuf>,
             met_target: bool|
             -> Result<CompressionResult, CompressionError> {
                if cancel_token.is_cancelled() {
                    remove_all(&*temp_paths_for_build.borrow());
                    return Err(CompressionError::Cancelled);
                }
                let attempt_path = temp_dir.join(format!("attempt-{quality}.{ext}"));
                temp_paths_for_build.borrow_mut().push(attempt_path.clone());
                write_archive(input, &metas, &chosen, &attempt_path)?;
                let size = std::fs::metadata(&attempt_path)
                    .map_err(|e| output_file_error(&attempt_path, e))?
                    .len();
                let strategy = if met_target {
                    format!("quality={}", quality)
                } else {
                    format!("quality={},best-effort", quality)
                };
                let pending = Ok((attempt_path, size, strategy, quality, met_target));
                deliver_or_cancel(
                    pending,
                    output,
                    &cancel_token,
                    &*temp_paths_for_build.borrow(),
                    &progress,
                )
            };

        match search {
            Ok((fit, effort)) => {
                if let Some((quality, _predicted, chosen)) = fit {
                    let result = build_and_deliver(quality, chosen, true)?;
                    if result.compressed_size <= target_bytes || quality <= min_quality {
                        return Ok(result);
                    }
                    let lower = (quality - 1).max(min_quality);
                    if lower == quality {
                        return Ok(result);
                    }
                    let (_, chosen_lower) = attempt(MAX_SEARCH_ITERATIONS, lower)?;
                    return build_and_deliver(lower, chosen_lower, false);
                }

                if let Some((quality, _predicted, chosen)) = effort {
                    return build_and_deliver(quality, chosen, false);
                }

                remove_all(&*temp_paths.borrow());
                Err(CompressionError::SidecarFailed(
                    "Could not compress document".to_string(),
                ))
            }
            Err(CompressionError::Cancelled) => {
                remove_all(&*temp_paths.borrow());
                Err(CompressionError::Cancelled)
            }
            Err(e) => {
                remove_all(&*temp_paths.borrow());
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Read, Write};
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

    fn entry_compression_method(path: &Path, name: &str) -> CompressionMethod {
        let file = fs::File::open(path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            if entry.name().unwrap().as_ref() == name {
                return entry.compression();
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
base="${out%%\[*]}"
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
    fn one_entry_is_decompressed_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/a.jpg", noise_bytes(500)),
                ("word/media/b.jpg", noise_bytes(500)),
                ("word/media/c.png", vec![9; 300]),
            ],
        );
        let metas = read_index(&input).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        OPEN_ENTRIES.with(|c| *c.borrow_mut() = 0);
        MAX_OPEN_ENTRIES.with(|c| *c.borrow_mut() = 0);
        extract_media(&input, &metas, &tmp).unwrap();
        assert_eq!(
            OPEN_ENTRIES.with(|c| *c.borrow()),
            0,
            "no zip entry may remain open after extraction"
        );
        assert_eq!(
            MAX_OPEN_ENTRIES.with(|c| *c.borrow()),
            1,
            "only one zip entry may be open at a time"
        );
        assert_eq!(
            fs::read_dir(&tmp).unwrap().count(),
            3,
            "all three media entries must be extracted to disk"
        );
    }

    #[cfg(unix)]
    #[test]
    fn extracted_media_is_cleaned_on_success_and_cancel() {
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

        DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                6144,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake.clone())),
                &tmp,
            )
            .unwrap();

        let remaining: Vec<_> = fs::read_dir(&tmp).unwrap().flatten().collect();
        assert!(
            remaining.is_empty(),
            "temp entries must be cleaned after success, found: {:?}",
            remaining.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );

        fs::remove_dir_all(&tmp).unwrap();
        fs::create_dir_all(&tmp).unwrap();
        let token = CancellationToken::new();
        let worker_token = token.clone();
        let worker_input = input.clone();
        let worker_output = dir.path().join("cancel.docx");
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

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if read_q_log(&dir.path()).len() >= 1 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "q.log never reached 1 entry"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        token.cancel();
        let result = handle.join().unwrap();
        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "cancel must surface, got {:?}",
            result.map(|r| (r.strategy_used, r.compressed_size))
        );
        let remaining: Vec<_> = fs::read_dir(&tmp).unwrap().flatten().collect();
        assert!(
            remaining.is_empty(),
            "temp entries must be cleaned after cancel, found: {:?}",
            remaining.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn non_media_entries_are_byte_identical() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.docx");
        let xml = varied_text(100);
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("_rels/.rels", RELS_XML.to_vec()),
                ("word/document.xml", xml.clone()),
                ("word/media/image1.jpg", noise_bytes(500)),
            ],
        );
        let metas = read_index(&input).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let media = extract_media(&input, &metas, &tmp).unwrap();
        let out = dir.path().join("out.docx");
        write_archive(&input, &metas, &media, &out).unwrap();

        let produced = read_zip_contents(&out);
        assert_eq!(produced[0].1.as_slice(), CONTENT_TYPES);
        assert_eq!(produced[1].1.as_slice(), RELS_XML);
        assert_eq!(produced[2].1, xml);
    }

    #[cfg(unix)]
    #[test]
    fn fixed_len_equals_scratch_file_size() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", varied_text(1000)),
                ("ppt/slides/slide1.xml", varied_text(500)),
                ("word/media/image1.jpg", noise_bytes(500)),
            ],
        );
        let metas = read_index(&input).unwrap();
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let fixed_len = measure_fixed_part(&input, &metas, &tmp).unwrap();
        // scratch file is removed by measure_fixed_part; read size by rerunning
        // the non-media phase directly and checking the counter and file size.
        let scratch = tmp.join("check.docx");
        let file = fs::File::create(&scratch).unwrap();
        let counter = CountingWriter::new(file);
        let mut writer = zip::ZipWriter::new(counter);
        let input_file = fs::File::open(&input).unwrap();
        let mut archive = zip::ZipArchive::new(input_file).unwrap();
        for meta in &metas {
            if meta.is_dir || media_disposition(&meta.name) != MediaDisposition::NotMedia {
                continue;
            }
            let mut entry = archive.by_index(meta.index).unwrap();
            writer
                .start_file(meta.name.clone(), deflate_options())
                .unwrap();
            std::io::copy(&mut entry, &mut writer).unwrap();
        }
        let counter = writer.finish().unwrap();
        let counted = counter.position();
        let file_len = fs::metadata(&scratch).unwrap().len();
        assert_eq!(
            counted, file_len,
            "CountingWriter position must equal the final scratch file size (counted={counted}, file={file_len})"
        );
        assert_eq!(
            fixed_len, file_len,
            "measure_fixed_part must return the same position"
        );
    }

    #[cfg(unix)]
    #[test]
    fn already_compressed_media_is_stored_not_deflated() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.jpg", noise_bytes(9000)),
                ("word/media/image2.png", vec![9; 500]),
                ("word/media/image3.jpeg", vec![7; 500]),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        DocumentProcessor
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

        let out = dir.path().join("out.docx");
        assert_eq!(
            entry_compression_method(&out, "word/document.xml"),
            CompressionMethod::Deflated,
            "XML must be deflated"
        );
        for name in [
            "word/media/image1.jpg",
            "word/media/image2.png",
            "word/media/image3.jpeg",
        ] {
            assert_eq!(
                entry_compression_method(&out, name),
                CompressionMethod::Stored,
                "{name} must be stored"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn media_never_grows() {
        let dir = tempfile::tempdir().unwrap();
        let fake = make_executable_script(
            dir.path(),
            "fake-vips-grow",
            r#"#!/bin/bash
out="$3"
base="${out%%\[*]}"
# Always emit 9500 bytes regardless of quality, larger than the 9000-byte original.
awk -v n=9500 'BEGIN { s = 12345; for (i = 0; i < n; i++) { s = (s * 16807 + 11) % 2147483647; printf "%c", int(s / 32768) % 255 + 1 } }' > "$base"
"#,
        );
        let input = dir.path().join("in.docx");
        let original = noise_bytes(9000);
        build_docx(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/document.xml", DOCUMENT_XML.to_vec()),
                ("word/media/image1.jpg", original.clone()),
            ],
        );
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        DocumentProcessor
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

        let output = read_zip_contents(&dir.path().join("out.docx"));
        let media_entry = output
            .iter()
            .find(|(n, _)| n == "word/media/image1.jpg")
            .expect("media entry must survive");
        assert_eq!(media_entry.1, original, "media must not grow");
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
    /// to create (duplicate entry names) and for crafting declared sizes.
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
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        buf.extend_from_slice(&(central.len() as u32).to_le_bytes());
        buf.extend_from_slice(&central_offset.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        fs::write(path, &buf).unwrap();
    }

    /// Same as `build_raw_stored_zip` but the central directory claims a
    /// custom uncompressed `declared_size` for every entry.
    fn build_raw_stored_zip_with_declared_sizes(
        path: &Path,
        entries: &[(&str, Vec<u8>, u64)],
    ) {
        let mut buf: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, content, declared_size) in entries {
            let offset = buf.len() as u32;
            let crc = crc32(content);
            let compressed_len = content.len() as u32;
            let declared = *declared_size as u32;
            buf.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
            buf.extend_from_slice(&20u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&crc.to_le_bytes());
            buf.extend_from_slice(&compressed_len.to_le_bytes());
            buf.extend_from_slice(&compressed_len.to_le_bytes());
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
            central.extend_from_slice(&compressed_len.to_le_bytes());
            central.extend_from_slice(&declared.to_le_bytes());
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
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        buf.extend_from_slice(&(central.len() as u32).to_le_bytes());
        buf.extend_from_slice(&central_offset.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        fs::write(path, &buf).unwrap();
    }

    #[test]
    fn zip_bomb_ratio_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("bomb.docx");
        let content = vec![b'x'; 1000];
        // Claim each of 250 entries decompresses to 1 MiB while stored as 1 KiB:
        // declared total 250 MiB / 2.5 MiB compressed > 200:1.
        let entries: Vec<(&str, Vec<u8>, u64)> = (0..250)
            .map(|i| (format!("part{i}.xml"), content.clone(), 1024 * 1024))
            .map(|(n, c, s)| (Box::leak(n.into_boxed_str()) as &str, c, s))
            .collect();
        build_raw_stored_zip_with_declared_sizes(&input, &entries);

        let err = read_index(&input).unwrap_err();
        assert!(
            matches!(err, CompressionError::InputFileError(ref m) if m.contains("ratio")),
            "ratio bomb must be rejected before extraction, got {err}"
        );
    }

    #[test]
    fn declared_uncompressed_total_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("huge.docx");
        let content = vec![b'x'; 100];
        let entries: Vec<(&str, Vec<u8>, u64)> = (0..2)
            .map(|i| (format!("part{i}.xml"), content.clone(), 3 * 1024 * 1024 * 1024))
            .map(|(n, c, s)| (Box::leak(n.into_boxed_str()) as &str, c, s))
            .collect();
        build_raw_stored_zip_with_declared_sizes(&input, &entries);

        let err = read_index(&input).unwrap_err();
        assert!(
            matches!(err, CompressionError::InputFileError(ref m) if m.contains("4 GiB")),
            "declared total over 4 GiB must be rejected before extraction, got {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn archive_is_written_once_per_accepted_quality() {
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
        FINISH_COUNT.with(|c| *c.borrow_mut() = 0);

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

        assert!(result.met_target, "strategy: {}", result.strategy_used);
        assert_eq!(
            FINISH_COUNT.with(|c| *c.borrow()),
            1,
            "exactly one archive finish per accepted quality"
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
    fn document_media_path_identification() {
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
                6000,
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

        let file = fs::File::open(&out_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            let name = entry.name().unwrap();
            let method = entry.compression();
            if name == "word/document.xml" || name == "[Content_Types].xml" {
                assert_eq!(method, CompressionMethod::Deflated, "{name} must be deflated");
            } else {
                assert_eq!(method, CompressionMethod::Stored, "{name} media must be stored");
            }
        }

        let ref9 = dir.path().join("ref9.zip");
        let ref6 = dir.path().join("ref6.zip");
        write_reference_zip(
            &ref9,
            &[("word/document.xml", doc_xml.clone())],
            Some(DEFLATE_LEVEL),
        );
        write_reference_zip(
            &ref6,
            &[("word/document.xml", doc_xml.clone())],
            None,
        );

        let produced_doc = entry_compressed_size(&out_path, "word/document.xml");
        let level9_doc = entry_compressed_size(&ref9, "word/document.xml");
        let level6_doc = entry_compressed_size(&ref6, "word/document.xml");
        assert_eq!(
            produced_doc, level9_doc,
            "word/document.xml must be deflated at level 9"
        );
        assert_ne!(
            produced_doc, level6_doc,
            "varied text must compress differently at level 9 vs 6"
        );

        let media_original = produced.iter().find(|(n, _)| n == "word/media/image1.jpg").unwrap().1.clone();
        let media_recompressed = produced
            .iter()
            .find(|(n, _)| n == "word/media/image1.jpg")
            .unwrap()
            .1
            .len();
        assert!(
            media_recompressed <= media_original.len(),
            "media must not grow"
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

        let tmp = dir.path().join("tmp40");
        fs::create_dir_all(&tmp).unwrap();
        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out40.docx"),
                100,
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

        let raw_log = fs::read_to_string(dir.path().join("q.log")).unwrap_or_default();
        assert!(
            !raw_log.is_empty(),
            "vips must have been invoked (and failed) for the media"
        );
        let output = read_zip_contents(&dir.path().join("out.docx"));
        assert_eq!(output.len(), 3, "the failed media entry must not be dropped");
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
        let fake = make_executable_script(
            dir.path(),
            "fake-vips-sleep",
            r#"#!/bin/bash
out="$3"
base="${out%%\[*]}"
q="${out#*\[Q=}"; q="${q%,*}"
echo "$q" >> "$(dirname "$0")/q.log"
case "$base" in
  *media-1-q67*) sleep 5 ;;
esac
n=$(( q * 100 ))
awk -v n="$n" 'BEGIN { for (i = 0; i < n; i++) printf "%c", i % 251 }' > "$base"
"#,
        );
        let input = dir.path().join("in.docx");
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

    #[test]
    #[cfg(unix)]
    fn document_cancel_during_rezip_no_clobber_existing_output() {
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

        let output_path = dir.path().join("out.docx");
        fs::write(&output_path, b"precious-unrelated-content").unwrap();

        let result = DocumentProcessor.compress_to_target(
            &input,
            &output_path,
            100,
            &floors_with(40),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(fake)),
            &tmp,
        );

        assert!(
            matches!(result, Err(CompressionError::OutputFileError(_))),
            "pre-existing output must not be clobbered, got {:?}",
            result.map(|r| (r.strategy_used, r.compressed_size))
        );
        assert_eq!(
            fs::read(&output_path).unwrap(),
            b"precious-unrelated-content",
            "existing output must remain untouched (Task 1 no-clobber)"
        );
        let remaining: Vec<_> = fs::read_dir(&tmp).unwrap().flatten().collect();
        assert!(
            remaining.is_empty(),
            "temp entries must be cleaned after no-clobber failure, found: {:?}",
            remaining.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }

    #[test]
    #[cfg(unix)]
    fn document_cancel_during_zero_media_rezip_no_output_no_temp() {
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
            input_size > 100 * 1024,
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

        wait_for_file(&tmp, "fixed.docx");
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
    /// 9000 bytes; q ≤ 54 → 9000 bytes; only 55..=64 → 6000 bytes.
    #[cfg(unix)]
    fn write_fake_vips_bump(dir: &Path) -> String {
        make_executable_script(
            dir,
            "fake-vips-bump",
            r#"#!/bin/bash
out="$3"
base="${out%%\[*]}"
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

        let output = read_zip_contents(&dir.path().join("out.docx"));
        let media_entry = output
            .iter()
            .find(|(n, _)| n == "word/media/image1.jpg")
            .expect("media entry must survive");
        assert_eq!(
            media_entry.1,
            noise_bytes(6000),
            "delivered media must be the SMALLEST attempt's bytes"
        );
        assert!(
            result.compressed_size <= 7000,
            "delivered size {} must track the smallest attempt",
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
base="${out%%\[*]}"
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
base="${out%%\[*]}"
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
    fn document_duplicate_entry_names_keep_first_copy_without_crash() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("in.docx");
        let first_media = noise_bytes(8000);
        build_raw_stored_zip(
            &input,
            &[
                ("[Content_Types].xml", CONTENT_TYPES.to_vec()),
                ("word/media/image1.jpg", first_media.clone()),
                ("word/media/image1.jpg", b"second copy".to_vec()),
            ],
        );
        let input_bytes = fs::read(&input).unwrap();
        let input_size = input_bytes.len() as u64;
        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

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

        // zip 9.x reads the central directory and exposes only one entry per
        // name, so the processor sees a normal two-entry archive.
        let result = result.unwrap();
        assert!(
            !result.strategy_used.is_empty(),
            "strategy must be reported: {}",
            result.strategy_used
        );
        let output = read_zip_contents(&dir.path().join("out.docx"));
        assert_eq!(
            output.len(),
            2,
            "zip 9.x deduplicates names; only the first copy survives"
        );
        let media_entry = output
            .iter()
            .find(|(n, _)| n == "word/media/image1.jpg")
            .expect("media entry must exist");
        assert!(
            media_entry.1.len() <= first_media.len(),
            "kept media must not be larger than the first copy"
        );
        assert_eq!(
            fs::read(&input).unwrap(),
            input_bytes,
            "original file must be preserved"
        );
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
            "empty archive must exercise the REZIP path"
        );
        assert!(
            !result.met_target,
            "21-byte target cannot fit the 22-byte rezipped empty archive"
        );
        assert_eq!(
            result.compressed_size, input_size,
            "empty archive rezipped must stay byte-identical in size"
        );
        let out_path = dir.path().join("out.docx");
        assert_eq!(fs::metadata(&out_path).unwrap().len(), input_size);
        assert!(
            zip::ZipArchive::new(fs::File::open(&out_path).unwrap()).is_ok(),
            "round-trip must still be a readable zip"
        );
    }

    /// Real-vips end-to-end smoke (requires real vips sidecar).
    #[test]
    #[cfg(unix)]
    #[ignore = "requires real vips sidecar on this machine"]
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

    #[test]
    #[cfg(unix)]
    #[ignore = "large-file smoke: allocates a 256 MiB zip"]
    fn document_streams_a_256_mib_archive() {
        let dir = tempfile::tempdir().unwrap();
        let fake = write_fake_vips(&dir.path());
        let input = dir.path().join("big.docx");

        let file = fs::File::create(&input).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file(
                "[Content_Types].xml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(CONTENT_TYPES).unwrap();
        writer
            .start_file(
                "word/document.xml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(DOCUMENT_XML).unwrap();

        let big_media = noise_bytes(256 * 1024 * 1024);
        writer
            .start_file(
                "word/media/image1.jpg",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(&big_media).unwrap();
        writer.finish().unwrap();

        let tmp = dir.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();

        let result = DocumentProcessor
            .compress_to_target(
                &input,
                &dir.path().join("out.docx"),
                128 * 1024 * 1024,
                &floors_with(40),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake)),
                &tmp,
            )
            .unwrap();

        let output = read_zip_contents(&dir.path().join("out.docx"));
        assert_eq!(output.len(), 3, "entry count must be preserved");
        assert!(
            result.compressed_size <= 128 * 1024 * 1024 || !result.met_target,
            "result must be honest about target satisfaction"
        );
    }
}
