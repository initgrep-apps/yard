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
use std::path::Path;

use tokio_util::sync::CancellationToken;

use crate::engine::processor::run_sidecar_capture_stdout;
use crate::engine::{
    AttemptDecision, AttemptOutcome, CompressionError, CompressionProcessor, CompressionResult,
    JobContext, Ladder, SidecarName, ToleratedExit, gs_failure_with_resources_hint, run_sidecar,
    strict_exit,
};
use crate::models::Category;
use crate::policy::Change;
use crate::settings::QualityFloors;

pub struct PdfProcessor;

const PDF_MAGIC: &[u8] = b"%PDF-";
const QPDF_JPEG_QUALITY: u32 = 75;
const PDF_DPI_RUNGS: [u32; 4] = [100, 72, 50, 36];
const PDF_JPEGQ_RUNGS: [u32; 4] = [60, 40, 25, 15];

/// qpdf exit 3 = "warnings, output still valid"; exit 2 = "errors" and must
/// fail the pass. A tolerated warning exit still goes through `measure_output`
/// and `validate_pdf`, so an unusable output cannot be delivered.
fn qpdf_tolerated_exit(label: &str, code: Option<i32>) -> bool {
    label == "qpdf" && code == Some(3)
}

impl Default for PdfProcessor {
    fn default() -> Self {
        Self::new()
    }
}

enum PdfPass {
    QpdfLossless,
    QpdfImageOpt,
    GsLadder { dpi: u32, jpegq: u32 },
}

struct PassSpec {
    label: &'static str,
    args: Vec<String>,
    strategy: String,
    quality: u32,
}

impl PdfProcessor {
    pub fn new() -> Self {
        PdfProcessor
    }

    fn pass_changes(pass: &PdfPass) -> Vec<Change> {
        match pass {
            PdfPass::QpdfLossless => Vec::new(),
            PdfPass::QpdfImageOpt | PdfPass::GsLadder { .. } => vec![Change::LossyEncode],
        }
    }

    fn qpdf_lossless_args(input: &Path, output: &Path) -> Vec<String> {
        vec![
            "--linearize".to_string(),
            "--object-streams=generate".to_string(),
            "--compression-level=9".to_string(),
            input.display().to_string(),
            output.display().to_string(),
        ]
    }

    fn qpdf_image_opt_args(input: &Path, output: &Path) -> Vec<String> {
        vec![
            "--linearize".to_string(),
            "--object-streams=generate".to_string(),
            "--recompress-flate".to_string(),
            "--compression-level=9".to_string(),
            "--optimize-images".to_string(),
            format!("--jpeg-quality={}", QPDF_JPEG_QUALITY),
            input.display().to_string(),
            output.display().to_string(),
        ]
    }

    /// Builds the gs argument vector. When the staged resource tree resolved, the
    /// `-I` search paths (and the ICC profile dir) are prepended ahead of every
    /// engine flag so Ghostscript never falls back to its compiled-in Homebrew
    /// prefix; with no staged tree the vector is exactly the pre-resources one.
    ///
    /// The ICC dir goes in as `-uICCProfilesDir` (a user parameter, which is how
    /// `Resource/Init/gs_lev2.ps` reads it). `-sICCProfilesDir` is not accepted by
    /// pdfwrite: gs 10.08 aborts device init with "Unable to open the initial
    /// device" whenever it is passed.
    ///
    /// `-dPDFSETTINGS=/screen` pins every image resolution to 72 dpi, so `-r`
    /// alone cannot carry the DPI ladder: it changes the rasterisation
    /// resolution of vector content, not the downsampling of embedded images.
    /// The explicit `ColorImageResolution`/`GrayImageResolution`/
    /// `MonoImageResolution` flags make each rung actually re-sample.
    fn gs_args(
    dpi: Option<u32>,
    jpegq: Option<u32>,
    input: &Path,
    output: &Path,
    resources: Option<&crate::sidecar::GsResources>,
) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(resources) = resources {
            for search_path in &resources.search_paths {
                args.push(format!("-I{}", search_path));
            }
            if let Some(icc) = &resources.icc_profiles_dir {
                args.push(format!("-uICCProfilesDir={}", icc));
            }
        }
        args.extend([
            "-sDEVICE=pdfwrite".to_string(),
            "-dPDFSETTINGS=/screen".to_string(),
        ]);
        if let Some(dpi) = dpi {
            args.extend([
                format!("-r{}", dpi),
                format!("-dColorImageResolution={}", dpi),
                format!("-dGrayImageResolution={}", dpi),
                format!("-dMonoImageResolution={}", dpi),
            ]);
        }
        if let Some(jpegq) = jpegq {
            args.push(format!("-dJPEGQ={}", jpegq));
        }
        args.extend([
            "-dNOPAUSE".to_string(),
            "-dQUIET".to_string(),
            "-dBATCH".to_string(),
            format!("-sOutputFile={}", output.display()),
            input.display().to_string(),
        ]);
        args
    }

    fn pass_spec(
        pass: &PdfPass,
        input: &Path,
        output: &Path,
        resources: Option<&crate::sidecar::GsResources>,
    ) -> PassSpec {
        match pass {
            PdfPass::QpdfLossless => PassSpec {
                label: "qpdf",
                args: Self::qpdf_lossless_args(input, output),
                strategy: "qpdf-lossless".to_string(),
                quality: 100,
            },
            PdfPass::QpdfImageOpt => PassSpec {
                label: "qpdf",
                args: Self::qpdf_image_opt_args(input, output),
                strategy: format!("qpdf-image-opt,lossy,jpeg-quality={}", QPDF_JPEG_QUALITY),
                quality: QPDF_JPEG_QUALITY,
            },
            PdfPass::GsLadder { dpi, jpegq } => PassSpec {
                label: "gs",
                args: Self::gs_args(Some(*dpi), Some(*jpegq), input, output, resources),
                strategy: format!("gs dpi={},jpegq={}", dpi, jpegq),
                quality: *jpegq,
            },
        }
    }

    /// A sidecar exit 0 is not success on its own — an empty or non-PDF
    /// output must never copy through as Done (spec §6). The output must be
    /// non-empty and start with the `%PDF-` header.
    fn measure_output(path: &Path) -> Result<u64, CompressionError> {
        let mut file = std::fs::File::open(path).map_err(|e| {
            CompressionError::OutputFileError(format!("attempt output ({}): {e}", path.display()))
        })?;
        let mut head = Vec::new();
        (&mut file)
            .take(PDF_MAGIC.len() as u64)
            .read_to_end(&mut head)
            .map_err(|e| {
                CompressionError::OutputFileError(format!(
                    "attempt output ({}): {e}",
                    path.display()
                ))
            })?;
        if head.as_slice() != PDF_MAGIC {
            return Err(CompressionError::SidecarFailed(
                "File may be corrupt (sidecar output is not a readable PDF)".to_string(),
            ));
        }
        let size = file.metadata().map(|m| m.len()).map_err(|e| {
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

    fn ladder_passes(quality_floor: &QualityFloors) -> Vec<PdfPass> {
        let mut passes = vec![PdfPass::QpdfLossless, PdfPass::QpdfImageOpt];
        let min_dpi = quality_floor.pdf.min_dpi;
        let min_jpegq = quality_floor.pdf.min_jpeg_quality;
        for dpi in PDF_DPI_RUNGS {
            for jpegq in PDF_JPEGQ_RUNGS {
                if dpi >= min_dpi && jpegq >= min_jpegq {
                    passes.push(PdfPass::GsLadder { dpi, jpegq });
                }
            }
        }
        passes
    }
}

fn page_count(path: &Path, qpdf: &str) -> Result<u32, CompressionError> {
    let args = vec!["--show-npages".to_string(), path.display().to_string()];
    let (_, stdout, stderr) = run_sidecar_capture_stdout(
        qpdf,
        "qpdf",
        &args,
        &CancellationToken::new(),
        qpdf_tolerated_exit,
    )?;
    stdout.trim().parse::<u32>().map_err(|_| {
        CompressionError::SidecarFailed(format!(
            "unparsable page count for {}: {:?} (qpdf: {})",
            path.display(),
            stdout.trim(),
            stderr.trim()
        ))
    })
}

fn validate_pdf(input: &Path, output: &Path, qpdf: &str) -> Result<(), CompressionError> {
    let expected = page_count(input, qpdf)?;
    let actual = page_count(output, qpdf)?;
    if expected != actual {
        return Err(CompressionError::SidecarFailed(format!(
            "page count mismatch: input has {expected} pages, output has {actual}"
        )));
    }
    Ok(())
}

impl CompressionProcessor for PdfProcessor {
    fn category(&self) -> Category {
        Category::Pdf
    }

    fn supported_extensions(&self) -> &[&str] {
        crate::registry::extensions_for(Category::Pdf)
    }

    fn declared_changes(&self) -> &'static [Change] {
        &[Change::LossyEncode, Change::Rasterize]
    }

    fn compress(&self, ctx: &JobContext<'_>) -> Result<CompressionResult, CompressionError> {
        let input_size = std::fs::metadata(ctx.input)
            .map(|m| m.len())
            .map_err(|e| CompressionError::InputFileError(e.to_string()))?;

        let mut ladder = Ladder::new();
        if let Some(result) = ladder.shortcut(ctx, input_size)? {
            return Ok(result);
        }

        let qpdf_path = ctx.sidecars.require(SidecarName::Qpdf)?;
        let gs_path = ctx.sidecars.require(SidecarName::Gs)?;

        let passes = Self::ladder_passes(ctx.floors);
        let total = passes.len();
        let progress = ladder.progress(ctx);

        for (index, pass) in passes.iter().enumerate() {
            if ctx.cancel.is_cancelled() {
                return ladder.finish(ctx);
            }

            let temp_file = ctx.temp_dir.join(format!("attempt-{}.pdf", index));
            ladder.track(&temp_file);
            let gs_resources = ctx.sidecars.gs_resources.as_ref();
            let spec = Self::pass_spec(pass, ctx.input, &temp_file, gs_resources);
            let sidecar_path = match pass {
                PdfPass::QpdfLossless | PdfPass::QpdfImageOpt => qpdf_path,
                PdfPass::GsLadder { .. } => gs_path,
            };

            progress.send_progress(((index * 100) / total.max(1)) as u32);

            let attempt = run_sidecar(
                sidecar_path,
                spec.label,
                &spec.args,
                &ctx.cancel,
                match pass {
                    PdfPass::QpdfLossless | PdfPass::QpdfImageOpt => {
                        qpdf_tolerated_exit as ToleratedExit
                    }
                    PdfPass::GsLadder { .. } => strict_exit as ToleratedExit,
                },
            )
            .and_then(|()| Self::measure_output(&temp_file))
            .and_then(|size| validate_pdf(ctx.input, &temp_file, qpdf_path).map(|()| size));

            match attempt {
                Ok(size) => {
                    let changes = Self::pass_changes(pass);
                    let consent = ctx.consent_for(&changes);
                    let decision = ladder.record(
                        ctx,
                        AttemptOutcome::Produced {
                            path: temp_file,
                            size,
                            quality: spec.quality,
                            strategy: spec.strategy,
                            changes,
                            consent,
                        },
                    );
                    if let AttemptDecision::Stop(result) = decision {
                        return Ok(result);
                    }
                    if ctx.cancel.is_cancelled() {
                        return ladder.finish(ctx);
                    }
                }
                // Spec §5: one failing pass must not abort the remaining
                // passes — a corrupt-input failure in qpdf still leaves the
                // gs ladder a chance to succeed. Record the attempt as
                // failed and move on (issue 13).
                Err(error) => {
                    if ctx.cancel.is_cancelled() {
                        return ladder.finish(ctx);
                    }
                    eprintln!("warn: {} pass failed, trying next: {error}", spec.label);
                    let error = if spec.label == "gs" && gs_resources.is_none() {
                        gs_failure_with_resources_hint(error)
                    } else {
                        error
                    };
                    ladder.record(ctx, AttemptOutcome::Failed(error));
                    if ctx.cancel.is_cancelled() {
                        return ladder.finish(ctx);
                    }
                }
            }
        }

        ladder.finish(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{ProgressSender, SidecarPaths};
    use std::path::PathBuf;
    use crate::sidecar::GsResources;
    use std::fs;
    use std::time::{Duration, Instant};

    fn gs_resources_fixture() -> GsResources {
        GsResources {
            search_paths: vec![
                "/app/Resources/gs-resources/Resource/Init".to_string(),
                "/app/Resources/gs-resources/lib".to_string(),
                "/app/Resources/gs-resources/Resource/Font".to_string(),
                "/app/Resources/gs-resources/fonts".to_string(),
            ],
            icc_profiles_dir: Some("/app/Resources/gs-resources/iccprofiles".to_string()),
        }
    }

    #[test]
    fn gs_args_prepend_resource_flags_in_order() {
        let args = PdfProcessor::gs_args(
            Some(72),
            Some(25),
            Path::new("/tmp/in.pdf"),
            Path::new("/tmp/out.pdf"),
            Some(&gs_resources_fixture()),
        );

        let expected_prefix = vec![
            "-I/app/Resources/gs-resources/Resource/Init".to_string(),
            "-I/app/Resources/gs-resources/lib".to_string(),
            "-I/app/Resources/gs-resources/Resource/Font".to_string(),
            "-I/app/Resources/gs-resources/fonts".to_string(),
            "-uICCProfilesDir=/app/Resources/gs-resources/iccprofiles".to_string(),
            "-sDEVICE=pdfwrite".to_string(),
        ];
        assert_eq!(
            args[..expected_prefix.len()],
            expected_prefix[..],
            "resource flags must precede -sDEVICE=pdfwrite so gs never reads its compiled-in prefix"
        );
        assert_eq!(
            args[expected_prefix.len()],
            "-dPDFSETTINGS=/screen".to_string(),
            "the engine flags that follow are unchanged"
        );
        assert!(
            !args.iter().any(|a| a.starts_with("-sICCProfilesDir=")),
            "-sICCProfilesDir aborts pdfwrite device init in gs 10.08 — the user \
             parameter -uICCProfilesDir is the switch gs_lev2.ps reads: {:?}",
            args
        );
    }

    #[test]
    fn gs_args_omit_icc_profiles_when_absent() {
        let resources = GsResources {
            search_paths: vec!["/app/Resources/gs-resources/Resource/Init".to_string()],
            icc_profiles_dir: None,
        };
        let args = PdfProcessor::gs_args(
            None,
            None,
            Path::new("/tmp/in.pdf"),
            Path::new("/tmp/out.pdf"),
            Some(&resources),
        );

        assert_eq!(
            args[0],
            "-I/app/Resources/gs-resources/Resource/Init".to_string()
        );
        assert_eq!(args[1], "-sDEVICE=pdfwrite".to_string());
        assert!(
            !args.iter().any(|a| a.starts_with("-uICCProfilesDir=")),
            "no iccprofiles dir means no ICC profile flag: {:?}",
            args
        );
    }

    #[test]
    fn gs_args_never_emit_empty_resource_flag_set() {
        let empty = GsResources {
            search_paths: Vec::new(),
            icc_profiles_dir: None,
        };
        let without = PdfProcessor::gs_args(
            None,
            None,
            Path::new("/tmp/in.pdf"),
            Path::new("/tmp/out.pdf"),
            None,
        );
        let from_empty = PdfProcessor::gs_args(
            None,
            None,
            Path::new("/tmp/in.pdf"),
            Path::new("/tmp/out.pdf"),
            Some(&empty),
        );
        assert_eq!(
            from_empty, without,
            "an empty search path list must not prepend stray -I flags"
        );
    }

    #[test]
    fn gs_args_without_resources_match_today() {
        let args = PdfProcessor::gs_args(
            Some(100),
            Some(40),
            Path::new("/tmp/in.pdf"),
            Path::new("/tmp/out.pdf"),
            None,
        );

        assert_eq!(
            args,
            vec![
                "-sDEVICE=pdfwrite".to_string(),
                "-dPDFSETTINGS=/screen".to_string(),
                "-r100".to_string(),
                "-dColorImageResolution=100".to_string(),
                "-dGrayImageResolution=100".to_string(),
                "-dMonoImageResolution=100".to_string(),
                "-dJPEGQ=40".to_string(),
                "-dNOPAUSE".to_string(),
                "-dQUIET".to_string(),
                "-dBATCH".to_string(),
                "-sOutputFile=/tmp/out.pdf".to_string(),
                "/tmp/in.pdf".to_string(),
            ],
            "with no staged resources the argument vector is byte-identical to the pre-resources one"
        );
    }

    #[test]
    fn gs_args_pin_each_image_resolution_to_the_dpi_rung() {
        let args = PdfProcessor::gs_args(
            Some(50),
            Some(25),
            Path::new("/tmp/in.pdf"),
            Path::new("/tmp/out.pdf"),
            None,
        );

        for flag in [
            "-r50",
            "-dColorImageResolution=50",
            "-dGrayImageResolution=50",
            "-dMonoImageResolution=50",
        ] {
            assert!(
                args.contains(&flag.to_string()),
                "missing {flag} in {args:?}"
            );
        }
        let rung = args.iter().position(|a| a == "-r50").unwrap();
        assert!(
            args[rung + 1..rung + 4]
                .iter()
                .all(|a| a.contains("ImageResolution=50")),
            "image resolutions must follow -r and share its rung: {args:?}"
        );
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
    fn test_sidecar_paths(qpdf: Option<String>, gs: Option<String>) -> SidecarPaths {
        SidecarPaths {
            vips: None,
            ffmpeg: None,
            ffprobe: None,
            gs,
            qpdf,
            gs_resources: None,
        }
    }

    #[cfg(unix)]
    fn qpdf_compression_lines(dir: &Path) -> Vec<String> {
        fs::read_to_string(dir.join("qpdf.log"))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.contains("--show-npages"))
            .map(str::to_string)
            .collect()
    }

    #[cfg(unix)]
    fn fake_qpdf_with_pages(
        dir: &Path,
        name: &str,
        pass1_bytes: usize,
        pass2_bytes: usize,
        input_pages: &str,
        output_pages: &str,
    ) -> String {
        make_executable_script(
            dir,
            name,
            &format!(
                r#"#!/bin/bash
echo "qpdf $*" >> "$(dirname "$0")/qpdf.log"
if [ "$1" = "--show-npages" ]; then
  if [ "$2" = "$(dirname "$0")/in.pdf" ]; then
    printf '{input_pages}\n'
  else
    printf '{output_pages}\n'
  fi
  exit 0
fi
out="${{@: -1}}"
printf '%%PDF-' > "$out"
case "$*" in
  *--optimize-images*) head -c $(( {pass2} - 5 )) /dev/zero >> "$out" ;;
  *) head -c $(( {pass1} - 5 )) /dev/zero >> "$out" ;;
esac
"#,
                pass1 = pass1_bytes,
                pass2 = pass2_bytes,
                input_pages = input_pages,
                output_pages = output_pages,
            ),
        )
    }

    #[cfg(unix)]
    fn fake_qpdf(dir: &Path, pass1_bytes: usize, pass2_bytes: usize) -> String {
        fake_qpdf_with_pages(dir, "fake-qpdf", pass1_bytes, pass2_bytes, "10", "10")
    }

    #[cfg(unix)]
    fn fake_qpdf_failing_with_pages(
        dir: &Path,
        message: &str,
        input_pages: &str,
        output_pages: &str,
    ) -> String {
        make_executable_script(
            dir,
            "fake-qpdf-fail",
            &format!(
                r#"#!/bin/bash
echo "qpdf $*" >> "$(dirname "$0")/qpdf.log"
if [ "$1" = "--show-npages" ]; then
  if [ "$2" = "$(dirname "$0")/in.pdf" ]; then
    printf '{input_pages}\n'
  else
    printf '{output_pages}\n'
  fi
  exit 0
fi
echo "{message}" >&2
exit 1
"#,
                message = message,
                input_pages = input_pages,
                output_pages = output_pages,
            ),
        )
    }

    #[cfg(unix)]
    fn fake_qpdf_failing(dir: &Path, message: &str) -> String {
        fake_qpdf_failing_with_pages(dir, message, "10", "10")
    }

    #[cfg(unix)]
    fn fake_qpdf_garbage_ok(dir: &Path, pass1_bytes: usize, pass2_bytes: usize) -> String {
        make_executable_script(
            dir,
            "fake-qpdf-garbage",
            &format!(
                r#"#!/bin/bash
echo "qpdf $*" >> "$(dirname "$0")/qpdf.log"
out="${{@: -1}}"
case "$*" in
  *--optimize-images*) head -c {pass2} /dev/urandom > "$out" ;;
  *) head -c {pass1} /dev/urandom > "$out" ;;
esac
exit 0
"#,
                pass1 = pass1_bytes,
                pass2 = pass2_bytes,
            ),
        )
    }

    #[cfg(unix)]
    fn fake_qpdf_warn_ok(dir: &Path, pass1_bytes: usize, pass2_bytes: usize) -> String {
        make_executable_script(
            dir,
            "fake-qpdf-warn",
            &format!(
                r#"#!/bin/bash
echo "qpdf $*" >> "$(dirname "$0")/qpdf.log"
if [ "$1" = "--show-npages" ]; then
  printf '10\n'
  exit 0
fi
out="${{@: -1}}"
printf '%%PDF-' > "$out"
case "$*" in
  *--optimize-images*) head -c $(( {pass2} - 5 )) /dev/zero >> "$out" ;;
  *) head -c $(( {pass1} - 5 )) /dev/zero >> "$out" ;;
esac
echo "WARNING: something recoverable happened" >&2
exit 3
"#,
                pass1 = pass1_bytes,
                pass2 = pass2_bytes,
            ),
        )
    }

    #[cfg(unix)]
    fn fake_gs(dir: &Path, screen_bytes: usize, ladder_multiplier: usize) -> String {
        make_executable_script(
            dir,
            "fake-gs",
            &format!(
                r#"#!/bin/bash
echo "gs $*" >> "$(dirname "$0")/gs.log"
out=""; dpi=""; q=""
for a in "$@"; do
  case "$a" in
    -sOutputFile=*) out="${{a#-sOutputFile=}}" ;;
    -r*) dpi="${{a#-r}}" ;;
    -dJPEGQ=*) q="${{a#-dJPEGQ=}}" ;;
  esac
done
case "$dpi" in
  "") {{ printf '%%PDF-'; head -c $(( {screen} - 5 )) /dev/zero; }} > "$out" ;;
  *) {{ printf '%%PDF-'; head -c $(( dpi * q * {mult} - 5 )) /dev/zero; }} > "$out" ;;
esac
"#,
                screen = screen_bytes,
                mult = ladder_multiplier,
            ),
        )
    }

    #[cfg(unix)]
    fn fake_gs_garbage(dir: &Path) -> String {
        make_executable_script(
            dir,
            "fake-gs-garbage",
            r#"#!/bin/bash
echo "gs $*" >> "$(dirname "$0")/gs.log"
out=""
for a in "$@"; do
  case "$a" in
    -sOutputFile=*) out="${a#-sOutputFile=}" ;;
  esac
done
head -c 2000 /dev/urandom > "$out"
exit 0
"#,
        )
    }

    #[cfg(unix)]
    fn fake_gs_zero_byte(dir: &Path) -> String {
        make_executable_script(
            dir,
            "fake-gs-zero",
            r#"#!/bin/bash
echo "gs $*" >> "$(dirname "$0")/gs.log"
out=""
for a in "$@"; do
  case "$a" in
    -sOutputFile=*) out="${a#-sOutputFile=}" ;;
  esac
done
: > "$out"
"#,
        )
    }

    /// gs exit 0 with a non-PDF payload (deterministic — no urandom timing).
    #[cfg(unix)]
    fn fake_gs_text_garbage(dir: &Path) -> String {
        make_executable_script(
            dir,
            "fake-gs-text",
            r#"#!/bin/bash
echo "gs $*" >> "$(dirname "$0")/gs.log"
out=""
for a in "$@"; do
  case "$a" in
    -sOutputFile=*) out="${a#-sOutputFile=}" ;;
  esac
done
printf 'not a pdf at all, just text' > "$out"
exit 0
"#,
        )
    }

    #[cfg(unix)]
    fn fake_gs_stderr_chatter_failer(dir: &Path) -> String {
        make_executable_script(
            dir,
            "fake-gs-chatter",
            r#"#!/bin/bash
echo "gs $*" >> "$(dirname "$0")/gs.log"
printf 'HEAD_START%3000d' 0 >&2
printf 'TAIL_END\n' >&2
exit 1
"#,
        )
    }

    #[cfg(unix)]
    fn fake_qpdf_sleeper(dir: &Path, pass1_bytes: usize, pass2_bytes: usize) -> String {
        make_executable_script(
            dir,
            "fake-qpdf-sleeper",
            &format!(
                r#"#!/bin/bash
echo "qpdf $*" >> "$(dirname "$0")/qpdf.log"
out="${{@: -1}}"
printf '%%PDF-' > "$out"
case "$*" in
  *--optimize-images*) head -c $(( {pass2} - 5 )) /dev/zero >> "$out"; sleep 60 ;;
  *) head -c $(( {pass1} - 5 )) /dev/zero >> "$out" ;;
esac
"#,
                pass1 = pass1_bytes,
                pass2 = pass2_bytes,
            ),
        )
    }

    #[cfg(unix)]
    fn oversized_input(dir: &Path) -> PathBuf {
        let input = dir.join("in.pdf");
        fs::write(&input, vec![0u8; 300 * 1024]).unwrap();
        input
    }

    #[cfg(unix)]
    fn temp_dir_under(dir: &Path) -> PathBuf {
        let tmp = dir.join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    #[test]
    #[cfg(unix)]
    fn pdf_pass1_qpdf_lossless_stops_early_when_under_target() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf(&dir.path(), 1000, 1000);
        let fake_gs = fake_gs(&dir.path(), 100, 100);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor
            .compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                2000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
                &tmp,
            ))
            .unwrap();

        assert!(result.met_target, "strategy: {}", result.strategy_used);
        assert_eq!(result.strategy_used, "qpdf-lossless");
        assert_eq!(result.compressed_size, 1000);

        let lines = qpdf_compression_lines(dir.path());
        assert_eq!(
            lines.len(),
            1,
            "pass 1 fitting must not run later passes: {:?}",
            lines
        );
        assert!(lines[0].contains("--compression-level=9"));
        assert!(lines[0].contains("--linearize"));
        assert!(lines[0].contains("--object-streams=generate"));
        assert!(
            !dir.path().join("gs.log").exists(),
            "gs must never spawn when pass 1 already fits"
        );
    }

    #[test]
    #[cfg(unix)]
    fn pdf_pass2_qpdf_image_opt_stops_early() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf(&dir.path(), 5000, 1000);
        let fake_gs = fake_gs(&dir.path(), 100, 1);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor
            .compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                2000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
                &tmp,
            ))
            .unwrap();

        assert!(result.met_target, "strategy: {}", result.strategy_used);
        assert!(result.strategy_used.contains("qpdf-image-opt"));
        assert!(result.strategy_used.contains("jpeg-quality=75"));
        assert_eq!(result.compressed_size, 1000);

        let lines = qpdf_compression_lines(dir.path());
        assert_eq!(lines.len(), 2, "got: {:?}", lines);
        assert!(
            lines[1].contains("--optimize-images") && lines[1].contains("--jpeg-quality=75"),
            "second invocation must be the image-opt pass: {}",
            lines[1]
        );
        assert!(
            !dir.path().join("gs.log").exists(),
            "gs must never run when pass 2 fits"
        );
    }

    const EXPECTED_GRID: [(u32, u32); 16] = [
        (100, 60),
        (100, 40),
        (100, 25),
        (100, 15),
        (72, 60),
        (72, 40),
        (72, 25),
        (72, 15),
        (50, 60),
        (50, 40),
        (50, 25),
        (50, 15),
        (36, 60),
        (36, 40),
        (36, 25),
        (36, 15),
    ];

    fn gs_cells(passes: &[PdfPass]) -> Vec<(u32, u32)> {
        passes
            .iter()
            .filter_map(|pass| match pass {
                PdfPass::GsLadder { dpi, jpegq } => Some((*dpi, *jpegq)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn rungs_are_ordered_by_aggressiveness() {
        let floors = QualityFloors {
            pdf: crate::settings::PdfQualityFloor {
                min_dpi: 36,
                min_jpeg_quality: 15,
            },
            ..QualityFloors::default()
        };
        let passes = PdfProcessor::ladder_passes(&floors);

        assert!(matches!(passes[0], PdfPass::QpdfLossless));
        assert!(matches!(passes[1], PdfPass::QpdfImageOpt));
        assert_eq!(
            passes.len(),
            2 + EXPECTED_GRID.len(),
            "lossless + image-opt + 16 grid cells, no separate 72 dpi screen rung"
        );

        let cells = gs_cells(&passes);
        assert_eq!(cells, EXPECTED_GRID.to_vec());
        for pair in cells.windows(2) {
            let ((dpi_a, q_a), (dpi_b, q_b)) = (pair[0], pair[1]);
            assert!(
                dpi_a > dpi_b || (dpi_a == dpi_b && q_a > q_b),
                "rungs must walk strictly decreasing dpi then decreasing JPEG quality: {pair:?}"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn gs_passes_reach_the_spawn_with_staged_resources() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf(&dir.path(), 5000, 5000);
        let fake_gs = fake_gs(&dir.path(), 1000, 1);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());
        let mut paths = test_sidecar_paths(Some(fake_qpdf), Some(fake_gs));
        paths.gs_resources = Some(GsResources {
            search_paths: vec!["/staged/Resource/Init".to_string(), "/staged/fonts".to_string()],
            icc_profiles_dir: Some("/staged/iccprofiles".to_string()),
        });

        let result = PdfProcessor
            .compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                2000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &paths,
                &tmp,
            ))
            .unwrap();

        assert_eq!(result.strategy_used, "gs dpi=100,jpegq=15");
        let gs_log = fs::read_to_string(dir.path().join("gs.log")).unwrap();
        let line = gs_log.lines().next().expect("the first gs cell ran");
        let flags: Vec<&str> = line.split_whitespace().skip(1).collect();
        assert_eq!(
            &flags[..4],
            &[
                "-I/staged/Resource/Init",
                "-I/staged/fonts",
                "-uICCProfilesDir=/staged/iccprofiles",
                "-sDEVICE=pdfwrite",
            ],
            "the spawned gs must receive the resource flags first: {line}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn pdf_dpi_jpegq_grid_walks_descending() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf(&dir.path(), 600_000, 600_000);
        let fake_gs = fake_gs(&dir.path(), 600_000, 1000);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let floors = QualityFloors {
            pdf: crate::settings::PdfQualityFloor {
                min_dpi: 36,
                min_jpeg_quality: 15,
            },
            ..QualityFloors::default()
        };

        let result = PdfProcessor
            .compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                2000,
                &floors,
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
                &tmp,
            ))
            .unwrap();

        assert!(
            !result.met_target,
            "nothing fits — must be best effort: {}",
            result.strategy_used
        );
        assert!(
            result.strategy_used.contains("best-effort"),
            "got: {}",
            result.strategy_used
        );
        assert_eq!(
            result.compressed_size, 540_000,
            "smallest grid cell is dpi=36 × jpegq=15 → 36*15*1000 bytes"
        );
        assert_eq!(
            result.quality_used, 15,
            "ladder quality_used is the JPEGQ rung, not the DPI rung"
        );

        let gs_log = fs::read_to_string(dir.path().join("gs.log")).unwrap();
        let gs_lines: Vec<&str> = gs_log.lines().collect();
        assert_eq!(
            gs_lines.len(),
            16,
            "16 grid cells with no separate screen rung: {:?}",
            gs_lines
        );
        assert_eq!(qpdf_compression_lines(dir.path()).len(), 2);

        let ladder: Vec<(u32, u32)> = gs_lines
            .iter()
            .filter(|l| l.contains("-r"))
            .map(|l| {
                let dpi = l
                    .split_whitespace()
                    .find(|a| a.starts_with("-r"))
                    .unwrap()[2..]
                    .parse()
                    .unwrap();
                let jpegq = l
                    .split_whitespace()
                    .find(|a| a.starts_with("-dJPEGQ="))
                    .unwrap()["-dJPEGQ=".len()..]
                    .parse()
                    .unwrap();
                (dpi, jpegq)
            })
            .collect();

        let expected: Vec<(u32, u32)> = [100u32, 72, 50, 36]
            .iter()
            .flat_map(|dpi| [60u32, 40, 25, 15].iter().map(move |q| (*dpi, *q)))
            .collect();
        assert_eq!(
            ladder, expected,
            "pass 4 must walk the enumerated grid in descending order"
        );
        for line in &gs_lines {
            assert!(
                line.contains("-dPDFSETTINGS=/screen"),
                "gs args must carry -dPDFSETTINGS: {}",
                line
            );
            assert!(
                line.contains("-sDEVICE=pdfwrite") && line.contains("-dNOPAUSE"),
                "got: {}",
                line
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn pdf_all_passes_fail_returns_best_effort_smallest_output() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf(&dir.path(), 5000, 4000);
        let fake_gs = fake_gs(&dir.path(), 6000, 100);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor
            .compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                2000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
                &tmp,
            ))
            .unwrap();

        assert!(!result.met_target);
        assert!(
            result.strategy_used.contains("best-effort"),
            "got: {}",
            result.strategy_used
        );
        assert_eq!(
            result.compressed_size, 4000,
            "best effort = SMALLEST output (qpdf image-opt), never last-wins"
        );
        assert_eq!(
            result.quality_used, QPDF_JPEG_QUALITY,
            "qpdf-image-opt best-effort keeps the jpeg-quality"
        );
        assert_eq!(
            fs::metadata(&dir.path().join("out.pdf")).unwrap().len(),
            4000
        );
    }

    #[test]
    #[cfg(unix)]
    fn pdf_already_under_target_skips_all_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf(&dir.path(), 1000, 1000);
        let fake_gs = fake_gs(&dir.path(), 100, 1);
        let input = dir.path().join("small.pdf");
        fs::write(&input, b"tiny pdf").unwrap();
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor
            .compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                2000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
                &tmp,
            ))
            .unwrap();

        assert!(result.met_target);
        assert_eq!(result.strategy_used, "already-under-target");
        assert_eq!(result.compressed_size, 8);
        assert_eq!(fs::read(&dir.path().join("out.pdf")).unwrap(), b"tiny pdf");
        assert!(
            !dir.path().join("qpdf.log").exists() && !dir.path().join("gs.log").exists(),
            "no sidecar may be spawned for an under-target pdf"
        );
    }

    #[test]
    #[cfg(unix)]
    fn pdf_gs_missing_binary_fails_with_engine_error() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf(&dir.path(), 1000, 1000);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor.compress(&JobContext::new(
            &input,
            &dir.path().join("out.pdf"),
            2000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(fake_qpdf), None),
            &tmp,
        ));
        match result {
            Err(e) => {
                assert!(
                    matches!(e, CompressionError::SidecarNotFound(ref n) if n == "gs"),
                    "expected SidecarNotFound(gs), got {:?}",
                    e
                );
                assert_eq!(e.to_string(), "Engine error: gs not available");
            }
            Ok(r) => panic!("expected error, got {:?}", r.strategy_used),
        }
        assert!(
            !dir.path().join("qpdf.log").exists(),
            "fail-fast: qpdf must not run when gs is already known missing"
        );
    }

    #[test]
    #[cfg(unix)]
    fn pdf_corrupt_input_sidecar_error_maps_to_file_may_be_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf_garbage_ok(&dir.path(), 1000, 1000);
        let fake_gs = fake_gs_text_garbage(&dir.path());
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor.compress(&JobContext::new(
            &input,
            &dir.path().join("out.pdf"),
            2000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
            &tmp,
        ));
        match result {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(
                    msg.contains("File may be corrupt"),
                    "exit-0 garbage bytes must never count as success, got: {}",
                    msg
                );
                assert!(
                    !dir.path().join("out.pdf").exists(),
                    "no invalid output may be delivered"
                );
            }
            Ok(r) => panic!("garbage output must be rejected, got {:?}", r.strategy_used),
            other => panic!("expected SidecarFailed, got {:?}", other.map(|r| r.strategy_used)),
        }
    }

    #[test]
    #[cfg(unix)]
    fn pdf_corrupt_gs_output_falls_back_to_earlier_valid_best_effort() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf(&dir.path(), 5000, 4000);
        let fake_gs = fake_gs_garbage(&dir.path());
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor
            .compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                2000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
                &tmp,
            ))
            .unwrap();

        assert!(
            !result.met_target,
            "corrupt gs output must not be delivered as Done (400 B fits target)"
        );
        assert!(
            result.strategy_used.contains("best-effort"),
            "gs pass errors must not abort the run: got {}",
            result.strategy_used
        );
        assert_eq!(
            result.compressed_size, 4000,
            "best effort = SMALLEST earlier valid output (qpdf image-opt)"
        );
    }

    #[test]
    #[cfg(unix)]
    fn pdf_qpdf_exit_three_warnings_output_still_measures() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf_warn_ok(&dir.path(), 1000, 1000);
        let fake_gs = fake_gs(&dir.path(), 100, 1);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor
            .compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                2000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
                &tmp,
            ))
            .unwrap();

        assert!(
            result.met_target,
            "qpdf exit 3 (warnings) keeps its output: strategy {}",
            result.strategy_used
        );
        assert_eq!(result.strategy_used, "qpdf-lossless");
        assert_eq!(result.compressed_size, 1000);
        assert!(
            !dir.path().join("gs.log").exists(),
            "pass-1 success (warnings included) must stop the ladder"
        );
    }

    #[test]
    #[cfg(unix)]
    fn run_sidecar_exit_semantics_are_per_engine() {
        let dir = tempfile::tempdir().unwrap();
        let warn = make_executable_script(
            &dir.path(),
            "sidecar-warn",
            r#"#!/bin/bash
echo "hello" >&2
exit 3
"#,
        );
        let fail2 = make_executable_script(
            &dir.path(),
            "sidecar-fail2",
            r#"#!/bin/bash
echo "hard error" >&2
exit 2
"#,
        );

        let cancelled = CancellationToken::new();
        assert!(
            run_sidecar(&warn, "qpdf", &[], &cancelled, qpdf_tolerated_exit).is_ok(),
            "qpdf exit 3 = warnings; output validity is decided by measure_output"
        );
        match run_sidecar(&fail2, "qpdf", &[], &cancelled, qpdf_tolerated_exit) {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(msg.contains("exited with code Some(2)"), "msg: {}", msg);
                assert!(
                    !msg.contains("File may be corrupt"),
                    "exit code alone must not claim corruption: {}",
                    msg
                );
            }
            other => panic!("expected failure, got {:?}", other.as_ref().map(|_| ())),
        }
        match run_sidecar(&warn, "gs", &[], &cancelled, strict_exit) {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(msg.contains("exited with code Some(3)"), "msg: {}", msg);
                assert!(
                    !msg.contains("File may be corrupt"),
                    "got: {}",
                    msg
                );
            }
            other => panic!("gs exit 3 is not a warning code, got {:?}", other.as_ref().map(|_| ())),
        }
    }

    #[test]
    fn qpdf_exit_three_is_a_warning_not_a_failure() {
        assert!(qpdf_tolerated_exit("qpdf", Some(3)));
        assert!(!qpdf_tolerated_exit("qpdf", Some(2)));
        assert!(!qpdf_tolerated_exit("qpdf", Some(0)));
        assert!(!qpdf_tolerated_exit("gs", Some(3)));
    }

    #[test]
    #[cfg(unix)]
    fn page_count_carries_the_qpdf_stderr_tail_into_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let fake = make_executable_script(
            &dir.path(),
            "fake-qpdf-encrypted",
            r#"#!/bin/bash
if [ "$1" = "--show-npages" ]; then
  echo "qpdf: invalid password" >&2
  exit 3
fi
exit 0
"#,
        );
        let input = dir.path().join("locked.pdf");
        fs::write(&input, b"%PDF-1.4").unwrap();

        let message = page_count(&input, &fake).unwrap_err().to_string();
        assert!(
            message.contains("invalid password"),
            "an encrypted PDF must surface qpdf's reason, not just a parse failure: {message}"
        );
    }

    #[test]
    fn floor_above_the_72_dpi_rung_keeps_only_100_dpi_cells() {
        let floors = QualityFloors {
            pdf: crate::settings::PdfQualityFloor {
                min_dpi: 81,
                min_jpeg_quality: 20,
            },
            ..QualityFloors::default()
        };

        let cells = gs_cells(&PdfProcessor::ladder_passes(&floors));
        assert_eq!(
            cells,
            vec![(100, 60), (100, 40), (100, 25)],
            "min_dpi=81 drops the 72 dpi cells and no dpi-less screen rung exists"
        );
    }

    #[test]
    #[cfg(unix)]
    fn pdf_cancel_mid_pass_returns_cancelled_and_cleans_temp() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf_sleeper(&dir.path(), 5000, 1000);
        let fake_gs = fake_gs(&dir.path(), 1000, 1);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let worker_input = input.clone();
        let worker_output = dir.path().join("out.pdf");
        let worker_tmp = tmp.clone();
        let worker_qpdf = fake_qpdf.clone();
        let worker_gs = fake_gs.clone();

        let token = CancellationToken::new();
        let worker_token = token.clone();
        let started = Instant::now();
        let handle = std::thread::spawn(move || {
            PdfProcessor.compress(&JobContext::new(
                &worker_input,
                &worker_output,
                2000,
                &QualityFloors::default(),
                worker_token,
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(worker_qpdf), Some(worker_gs)),
                &worker_tmp,
            ))
        });
        std::thread::sleep(Duration::from_millis(300));
        token.cancel();
        let result = handle.join().unwrap();
        let elapsed = started.elapsed();

        assert!(
            matches!(result, Err(CompressionError::Cancelled)),
            "cancel mid-pass must surface as Cancelled, got {:?}",
            result.map(|r| r.strategy_used)
        );
        assert!(
            elapsed < Duration::from_secs(10),
            "sidecar must be killed promptly, took {:?} (sleep was 60s)",
            elapsed
        );
        assert_eq!(
            fs::read_dir(&tmp).unwrap().count(),
            0,
            "temp attempt files must be cleaned after cancel"
        );
    }

    #[test]
    #[cfg(unix)]
    fn pdf_sidecar_stderr_tail_in_error_message() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf_failing(&dir.path(), "qpdf: input unreadable");
        let fake_gs = fake_gs_stderr_chatter_failer(&dir.path());
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor.compress(&JobContext::new(
            &input,
            &dir.path().join("out.pdf"),
            2000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
            &tmp,
        ));
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
            other => panic!("expected SidecarFailed, got {:?}", other.map(|r| r.strategy_used)),
        }
    }

    #[test]
    #[cfg(unix)]
    fn gs_pass_failure_surfaces_the_missing_resource_tree_only_when_unresolved() {
        let dir = tempfile::tempdir().unwrap();
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let without = PdfProcessor.compress(&JobContext::new(
            &input,
            &dir.path().join("out.pdf"),
            2000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(
                Some(fake_qpdf_failing(&dir.path(), "qpdf: cannot process")),
                Some(fake_gs_stderr_chatter_failer(&dir.path())),
            ),
            &tmp,
        ));
        match without {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(
                    msg.contains("exited with code Some(1)"),
                    "the original cause must survive: {}",
                    msg
                );
                assert!(
                    msg.contains("gs-resources/ tree is not available"),
                    "a gs pass with no -I flags must name the missing tree in the user-visible \
                     error, not only in eprintln: {}",
                    msg
                );
            }
            other => panic!("expected SidecarFailed, got {:?}", other.map(|r| r.strategy_used)),
        }

        let staged_gs = fake_gs_stderr_chatter_failer(&dir.path());
        let mut paths = test_sidecar_paths(
            Some(fake_qpdf_failing(&dir.path(), "qpdf: cannot process")),
            Some(staged_gs.clone()),
        );
        paths.gs_resources = Some(gs_resources_fixture());
        let with = PdfProcessor.compress(&JobContext::new(
            &input,
            &dir.path().join("out2.pdf"),
            2000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &paths,
            &tmp,
        ));
        match with {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(msg.contains("exited with code Some(1)"), "msg: {}", msg);
                assert!(
                    !msg.contains("gs-resources/ tree is not available"),
                    "with the tree staged the hint is noise and must not be emitted: {}",
                    msg
                );
            }
            other => panic!("expected SidecarFailed, got {:?}", other.map(|r| r.strategy_used)),
        }
    }

    #[test]
    #[cfg(unix)]
    fn pdf_zero_byte_output_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf_failing(&dir.path(), "qpdf: cannot process");
        let fake_gs = fake_gs_zero_byte(&dir.path());
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor.compress(&JobContext::new(
            &input,
            &dir.path().join("out.pdf"),
            2000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
            &tmp,
        ));
        match result {
            Err(e) => {
                assert!(
                    matches!(
                        e,
                        CompressionError::SidecarFailed(ref m)
                            if m.contains("File may be corrupt")
                    ),
                    "exit 0 + empty file must not count as success, got {:?}",
                    e
                );
            }
            Ok(r) => panic!("zero-byte output must be rejected, got {:?}", r.strategy_used),
        }
    }

    #[test]
    #[cfg(unix)]
    fn pdf_ladder_respects_settings_quality_floors() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf(&dir.path(), 5000, 5000);
        let fake_gs = fake_gs(&dir.path(), 5000, 1000);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor
            .compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                2000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
                &tmp,
            ))
            .unwrap();

        assert!(!result.met_target);
        let gs_log = fs::read_to_string(dir.path().join("gs.log")).unwrap();
        let ladder: Vec<u32> = gs_log
            .lines()
            .filter(|l| l.contains("-r"))
            .map(|l| {
                l.split_whitespace()
                    .find(|a| a.starts_with("-r"))
                    .unwrap()[2..]
                    .parse()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            ladder,
            vec![100, 100, 100, 100, 72, 72, 72, 72],
            "default floors (minDpi=72, minJpegQuality=15) truncate the ladder, not hardcoded"
        );
    }

    #[test]
    fn image_optimisation_is_marked_lossy() {
        let image_opt = PdfProcessor::pass_spec(
            &PdfPass::QpdfImageOpt,
            Path::new("/tmp/in.pdf"),
            Path::new("/tmp/out.pdf"),
            None,
        );
        let lossless = PdfProcessor::pass_spec(
            &PdfPass::QpdfLossless,
            Path::new("/tmp/in.pdf"),
            Path::new("/tmp/out.pdf"),
            None,
        );

        assert!(
            image_opt.strategy.contains("lossy"),
            "qpdf --optimize-images recompresses images and must be reported as lossy: {}",
            image_opt.strategy
        );
        assert!(
            !lossless.strategy.contains("lossy"),
            "the linearise pass is lossless: {}",
            lossless.strategy
        );
    }

    #[test]
    #[cfg(unix)]
    fn gs_output_with_fewer_pages_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf_failing_with_pages(&dir.path(), "qpdf: cannot process", "2", "1");
        let fake_gs = fake_gs(&dir.path(), 1000, 1);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor.compress(&JobContext::new(
            &input,
            &dir.path().join("out.pdf"),
            2000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
            &tmp,
        ));

        match result {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(
                    msg.contains("page count mismatch"),
                    "page loss must reject the attempt, got: {msg}"
                );
                assert!(
                    msg.contains('2') && msg.contains('1'),
                    "the mismatch must name both counts, got: {msg}"
                );
            }
            other => panic!(
                "an output with fewer pages must never be delivered, got {:?}",
                other.map(|r| r.strategy_used)
            ),
        }
        assert!(
            !dir.path().join("out.pdf").exists(),
            "no page-losing output may reach the destination"
        );

        let gs_runs = fs::read_to_string(dir.path().join("gs.log"))
            .unwrap()
            .lines()
            .count();
        assert!(
            gs_runs > 1,
            "a rejected rung must not stop the ladder, ran {gs_runs}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn page_count_probe_failure_rejects_the_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf_failing_with_pages(
            &dir.path(),
            "qpdf: cannot process",
            "2",
            "not-a-page-count",
        );
        let fake_gs = fake_gs(&dir.path(), 1000, 1);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let result = PdfProcessor.compress(&JobContext::new(
            &input,
            &dir.path().join("out.pdf"),
            2000,
            &QualityFloors::default(),
            CancellationToken::new(),
            ProgressSender::test_stub(),
            &test_sidecar_paths(Some(fake_qpdf), Some(fake_gs)),
            &tmp,
        ));

        match result {
            Err(CompressionError::SidecarFailed(msg)) => {
                assert!(
                    msg.contains("unparsable page count"),
                    "unparsable stdout is a failed rung, never a pass: {msg}"
                );
            }
            other => panic!(
                "an unverifiable output must not be delivered, got {:?}",
                other.map(|r| r.strategy_used)
            ),
        }
        assert!(!dir.path().join("out.pdf").exists());

        let gs_runs = fs::read_to_string(dir.path().join("gs.log"))
            .unwrap()
            .lines()
            .count();
        assert!(
            gs_runs > 1,
            "a failed probe must not stop the ladder, ran {gs_runs}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn missing_qpdf_and_missing_gs_fail_fast() {
        let dir = tempfile::tempdir().unwrap();
        let fake_qpdf = fake_qpdf(&dir.path(), 1000, 1000);
        let fake_gs = fake_gs(&dir.path(), 100, 1);
        let input = oversized_input(&dir.path());
        let tmp = temp_dir_under(&dir.path());

        let cases = [
            (None, Some(fake_gs.clone()), "qpdf"),
            (Some(fake_qpdf.clone()), None, "gs"),
            (None, None, "qpdf"),
        ];
        for (qpdf, gs, expected) in cases {
            let result = PdfProcessor.compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                2000,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(qpdf, gs),
                &tmp,
            ));
            match result {
                Err(CompressionError::SidecarNotFound(name)) => assert_eq!(
                    name, expected,
                    "the first missing engine must surface before any spawn"
                ),
                other => panic!(
                    "missing {expected} must fail fast, got {:?}",
                    other.map(|r| r.strategy_used)
                ),
            }
        }

        assert!(
            !dir.path().join("qpdf.log").exists() && !dir.path().join("gs.log").exists(),
            "fail-fast: no engine may run when a required one is missing"
        );
    }

    /// End-to-end smoke against the REAL qpdf + gs sidecars on this machine
    /// (ignored in CI-style runs; mirrors real_vips_compresses_oversized_jpeg_to_target).
    #[test]
    #[cfg(unix)]
    #[ignore = "requires real qpdf + gs sidecars on this machine"]
    fn real_pdf_sidecars_compress_oversized_pdf_through_all_passes() {
        let dir = tempfile::tempdir().unwrap();
        let binaries = format!("{}/binaries", env!("CARGO_MANIFEST_DIR"));
        let qpdf = format!(
            "{}/{}",
            binaries,
            crate::sidecar::sidecar_filename("qpdf").unwrap()
        );
        let gs = format!(
            "{}/{}",
            binaries,
            crate::sidecar::sidecar_filename("gs").unwrap()
        );
        assert!(Path::new(&qpdf).exists(), "qpdf sidecar missing: {}", qpdf);
        assert!(Path::new(&gs).exists(), "gs sidecar missing: {}", gs);

        let input = dir.path().join("in.pdf");
        let postscript: String = (1..=60)
            .map(|page| {
                format!(
                    "/Helvetica findfont 12 scalefont setfont\n72 700 moveto \
                     (Page {page}: The quick brown fox jumps over the lazy dog) show\nshowpage\n"
                )
            })
            .collect();
        let postscript_path = dir.path().join("in.ps");
        fs::write(&postscript_path, &postscript).unwrap();
        let output = std::process::Command::new(&gs)
            .args(["-sDEVICE=pdfwrite", "-dNOPAUSE", "-dQUIET", "-dBATCH"])
            .arg(format!("-sOutputFile={}", input.display()))
            .arg(&postscript_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .expect("gs generates the input pdf");
        assert!(
            output.status.success(),
            "gs failed to generate the input pdf"
        );

        let tmp = temp_dir_under(&dir.path());
        let result = PdfProcessor
            .compress(&JobContext::new(
                &input,
                &dir.path().join("out.pdf"),
                10 * 1024,
                &QualityFloors::default(),
                CancellationToken::new(),
                ProgressSender::test_stub(),
                &test_sidecar_paths(Some(qpdf), Some(gs)),
                &tmp,
            ))
            .unwrap();

        assert!(
            result.compressed_size > 0 && fs::metadata(&dir.path().join("out.pdf")).unwrap().len() > 0,
            "output must be a real file, strategy: {}",
            result.strategy_used
        );
    }

    /// A 4-inch page embedding the JPEG at 300 ppi at its native pixel size.
    #[cfg(unix)]
    fn write_full_page_jpeg_pdf(jpeg: &[u8], path: &Path) {
        let content = b"q 288 0 0 288 0 0 cm /Im0 Do Q\n";
        let objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 288 288] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>".to_vec(),
            format!(
                "<< /Type /XObject /Subtype /Image /Width 1200 /Height 1200 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
                jpeg.len()
            )
            .into_bytes()
            .into_iter()
            .chain(jpeg.iter().copied())
            .chain(b"\nendstream".iter().copied())
            .collect(),
            format!("<< /Length {} >>\nstream\n", content.len())
                .into_bytes()
                .into_iter()
                .chain(content.iter().copied())
                .chain(b"endstream".iter().copied())
                .collect(),
        ];

        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref
            )
            .as_bytes(),
        );
        fs::write(path, pdf).unwrap();
    }

    #[test]
    #[cfg(unix)]
    #[ignore = "requires real gs + qpdf sidecars on this machine"]
    fn real_gs_72_dpi_rung_downamples_an_embedded_300_ppi_image() {
        let dir = tempfile::tempdir().unwrap();
        let binaries = format!("{}/binaries", env!("CARGO_MANIFEST_DIR"));
        let gs = format!(
            "{}/{}",
            binaries,
            crate::sidecar::sidecar_filename("gs").unwrap()
        );
        let qpdf = format!(
            "{}/{}",
            binaries,
            crate::sidecar::sidecar_filename("qpdf").unwrap()
        );
        assert!(Path::new(&gs).exists(), "gs sidecar missing: {}", gs);
        assert!(Path::new(&qpdf).exists(), "qpdf sidecar missing: {}", qpdf);

        let jpeg = dir.path().join("noise.jpg");
        let generated = std::process::Command::new(&gs)
            .args([
                "-sDEVICE=jpeggray",
                "-r300",
                "-g1200x1200",
                "-dNOPAUSE",
                "-dBATCH",
            ])
            .arg(format!("-sOutputFile={}", jpeg.display()))
            .args(["-c", "0.5 setgray 0 0 1200 1200 rectfill showpage"])
            .output()
            .expect("gs generates the 300 ppi jpeg");
        assert!(
            generated.status.success(),
            "jpeg generation failed: {}",
            String::from_utf8_lossy(&generated.stderr)
        );

        let input = dir.path().join("in.pdf");
        write_full_page_jpeg_pdf(&fs::read(&jpeg).unwrap(), &input);
        let output = dir.path().join("out.pdf");
        let run = std::process::Command::new(&gs)
            .args(PdfProcessor::gs_args(
                Some(72),
                Some(40),
                &input,
                &output,
                None,
            ))
            .output()
            .expect("gs downsamples the pdf");
        assert!(
            run.status.success(),
            "gs failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );

        let qdf = dir.path().join("out-qdf.pdf");
        let converted = std::process::Command::new(&qpdf)
            .args(["--qdf", "--object-streams=disable"])
            .arg(&output)
            .arg(&qdf)
            .output()
            .expect("qpdf expands the output");
        assert!(
            converted.status.success(),
            "qpdf failed: {}",
            String::from_utf8_lossy(&converted.stderr)
        );

        let expanded = String::from_utf8_lossy(&fs::read(&qdf).unwrap()).into_owned();
        let image = expanded
            .split("/Subtype /Image")
            .nth(1)
            .expect("output must contain an image");
        let width: f64 = image
            .split("/Width ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|value| value.parse().ok())
            .expect("image width must be readable");
        let ppi = width / 4.0;
        assert!(
            (ppi - 72.0).abs() <= 3.0,
            "a 1200 px image on a 4-inch page must be downsampled near 72 ppi, got {ppi} ppi ({width} px)"
        );
    }
}
