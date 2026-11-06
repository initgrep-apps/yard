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

use std::path::PathBuf;

use crate::policy::{Change, ConsentSource};

pub const STRATEGY_ALREADY_UNDER_TARGET: &str = "already-under-target";

#[derive(Debug)]
pub struct CompressionResult {
    pub output_path: PathBuf,
    pub compressed_size: u64,
    pub quality_used: u32,
    pub met_target: bool,
    pub strategy_used: String,
    pub changes: Vec<Change>,
    pub consent: ConsentSource,
}

impl CompressionResult {
    pub fn is_already_under_target(&self) -> bool {
        self.strategy_used == STRATEGY_ALREADY_UNDER_TARGET
    }
}

#[derive(Debug, Clone)]
pub enum CompressionError {
    Cancelled,
    SidecarNotFound(String),
    SidecarFailed(String),
    InputFileError(String),
    OutputFileError(String),
    DiskFull,
    Unknown(String),
}

impl std::fmt::Display for CompressionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompressionError::Cancelled => write!(f, "Cancelled"),
            CompressionError::SidecarNotFound(name) => {
                write!(f, "Engine error: {} not available", name)
            }
            CompressionError::SidecarFailed(msg) => write!(f, "Processing error: {}", msg),
            CompressionError::InputFileError(msg) => write!(f, "Cannot read file: {}", msg),
            CompressionError::OutputFileError(msg) => write!(f, "Cannot write output: {}", msg),
            CompressionError::DiskFull => write!(f, "Not enough disk space"),
            CompressionError::Unknown(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for CompressionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_error_display_strings_match_spec_wording() {
        // (variant, expected) copy table — spec §6 wording is user-facing.
        // The exhaustive match below has NO wildcard arm, so adding an 8th
        // variant fails this build until the table is extended.
        let cases: Vec<(CompressionError, &str)> = vec![
            (CompressionError::Cancelled, "Cancelled"),
            (
                CompressionError::SidecarNotFound("vips".into()),
                "Engine error: vips not available",
            ),
            (
                CompressionError::SidecarFailed("boom".into()),
                "Processing error: boom",
            ),
            (
                CompressionError::InputFileError("locked".into()),
                "Cannot read file: locked",
            ),
            (
                CompressionError::OutputFileError("readonly".into()),
                "Cannot write output: readonly",
            ),
            (CompressionError::DiskFull, "Not enough disk space"),
            (CompressionError::Unknown("odd".into()), "odd"),
        ];
        assert_eq!(
            cases.len(),
            7,
            "copy table must cover every CompressionError variant"
        );
        for (i, (err, expected)) in cases.into_iter().enumerate() {
            match &err {
                CompressionError::Cancelled => {}
                CompressionError::SidecarNotFound(_) => {}
                CompressionError::SidecarFailed(_) => {}
                CompressionError::InputFileError(_) => {}
                CompressionError::OutputFileError(_) => {}
                CompressionError::DiskFull => {}
                CompressionError::Unknown(_) => {}
            }
            assert_eq!(err.to_string(), expected, "variant {i}");
        }
    }
}
