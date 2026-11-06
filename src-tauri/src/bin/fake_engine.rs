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

use std::io::Write;
use std::process::ExitCode;

fn arg_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn write_sized(path: &str, size: u64) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    let zeroes = [0u8; 8192];
    let mut left = size;
    while left > 0 {
        let chunk = left.min(zeroes.len() as u64) as usize;
        file.write_all(&zeroes[..chunk])?;
        left -= chunk as u64;
    }
    Ok(())
}

fn write_video(path: &str, size: u64) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(&[0, 0, 0, 0x18, b'f', b't', b'y', b'p'])?;
    let zeroes = [0u8; 8192];
    let mut left = size.saturating_sub(8);
    while left > 0 {
        let chunk = left.min(zeroes.len() as u64) as usize;
        file.write_all(&zeroes[..chunk])?;
        left -= chunk as u64;
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = std::env::var("FAKE_ENGINE_MODE").unwrap_or_default();

    match mode.as_str() {
        "sleep" => {
            let millis = args
                .last()
                .and_then(|arg| arg.strip_prefix("sleep-"))
                .and_then(|value| value.parse::<u64>().ok())
                .or_else(|| {
                    std::env::var("FAKE_ENGINE_SLEEP_MS")
                        .ok()
                        .and_then(|value| value.parse::<u64>().ok())
                })
                .unwrap_or(300);
            std::thread::sleep(std::time::Duration::from_millis(millis));
            return ExitCode::SUCCESS;
        }
        "fail" => return ExitCode::FAILURE,
        _ => {}
    }

    if args.iter().any(|arg| arg == "-show_entries") {
        let probe = std::env::var("FAKE_ENGINE_PROBE").unwrap_or_else(|_| {
            "duration=5.000000\ncodec_type=video\nwidth=640\ncodec_type=audio".to_string()
        });
        print!("{probe}\n");
        return ExitCode::SUCCESS;
    }

    if args.iter().any(|arg| arg == "-i") {
        if arg_after(&args, "-pass") == Some("1") {
            if let Some(prefix) = arg_after(&args, "-passlogfile") {
                let _ = write_sized(&format!("{prefix}-0.log"), 16);
                let _ = write_sized(&format!("{prefix}-0.log.mbtree"), 16);
            }
            return ExitCode::SUCCESS;
        }
        if let Some(output) = args.last() {
            if output != "NUL" && output != "/dev/null" {
                let size = std::env::var("FAKE_ENGINE_SIZE")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(100);
                let is_video = args
                    .windows(2)
                    .any(|pair| pair[0] == "-movflags" || pair[0] == "-pix_fmt");
                let result = if is_video {
                    write_video(output, size)
                } else {
                    write_sized(output, size)
                };
                return match result {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(_) => ExitCode::FAILURE,
                };
            }
        }
        return ExitCode::SUCCESS;
    }

    let op = args.first().map(String::as_str).unwrap_or("");
    if op == "thumbnail" || op == "copy" {
        let Some(output) = args.get(2) else {
            return ExitCode::FAILURE;
        };
        let base = output.split('[').next().unwrap_or(output);
        let width = args
            .get(3)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(100_000);
        let size = if width >= 100_000 {
            100_000
        } else {
            width * 25
        };
        return match write_sized(base, size) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }

    ExitCode::FAILURE
}
