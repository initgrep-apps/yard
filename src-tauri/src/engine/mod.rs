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

pub mod context;
pub mod ladder;
pub mod processor;
pub mod progress;
pub mod result;

pub use context::{ConsentDecision, ConsentGate, JobContext};
pub use ladder::{AttemptDecision, AttemptOutcome, Ladder};
pub use processor::{
    CompressionProcessor, SidecarName, SidecarPaths, StderrTail,
    ToleratedExit, gs_failure_with_resources_hint, run_sidecar, stderr_tail,
    strict_exit,
};
pub use progress::ProgressSender;

/// Test-only locator for the cross-platform fake engine. Cargo puts the
/// binary in `target/<profile>/` while unit-test executables live in
/// `target/<profile>/deps/`, so check beside the test binary first and fall
/// back one level. Panics when the binary is missing — a skipped test would
/// hide the platform coverage this exists to provide.
#[cfg(test)]
pub(crate) fn fake_engine_path() -> std::path::PathBuf {
    let name = if cfg!(windows) {
        "fake_engine.exe"
    } else {
        "fake_engine"
    };
    let exe = std::env::current_exe().expect("test executable path must resolve");
    let dir = exe
        .parent()
        .expect("test executable must have a parent directory");
    let direct = dir.join(name);
    if direct.exists() {
        return direct;
    }
    if let Some(parent) = dir.parent() {
        let fallback = parent.join(name);
        if fallback.exists() {
            return fallback;
        }
    }
    panic!(
        "fake_engine binary is missing; looked for {} and {}",
        direct.display(),
        dir.parent().map(|p| p.join(name).display().to_string()).unwrap_or_default()
    );
}
pub use result::{CompressionError, CompressionResult};
