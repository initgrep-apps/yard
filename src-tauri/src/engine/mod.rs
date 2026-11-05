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

pub mod processor;
pub mod progress;
pub mod result;

pub use processor::{
    BestAttempt, CompressionProcessor, SidecarName, SidecarPaths, StderrTail,
    ToleratedExit, deliver_output, gs_failure_with_resources_hint, run_sidecar, stderr_tail,
    strict_exit,
};
pub use progress::ProgressSender;
pub use result::{CompressionError, CompressionResult};
