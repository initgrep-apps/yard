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

use std::path::Path;
use std::sync::OnceLock;

use tokio_util::sync::CancellationToken;

use crate::engine::processor::SidecarPaths;
use crate::engine::progress::ProgressSender;
use crate::models::Category;
use crate::policy::{Change, ConsentSource, Policy, Verdict};
use crate::settings::QualityFloors;

fn default_policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(Policy::default)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsentDecision {
    Allow(ConsentSource),
    Deny(String),
    Cancelled,
}

/// Runs the request/response exchange for a consent-gated change. The queue
/// implements this over its pending-request registry; without a gate the
/// policy's static grants decide.
pub trait ConsentGate: Send + Sync {
    fn authorize(&self, category: Category, changes: &[Change]) -> ConsentDecision;
}

/// Everything a processor needs for one job. Values are resolved once by the
/// job body and handed over unchanged, so a processor never re-derives the
/// sidecar set, the temp tree, or the output path.
pub struct JobContext<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub target_bytes: u64,
    pub floors: &'a QualityFloors,
    pub cancel: CancellationToken,
    pub progress: ProgressSender,
    pub sidecars: &'a SidecarPaths,
    pub temp_dir: &'a Path,
    pub policy: &'a Policy,
    pub category: Category,
    pub declared: &'a [Change],
    pub consent_gate: Option<&'a dyn ConsentGate>,
}

impl<'a> JobContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        input: &'a Path,
        output: &'a Path,
        target_bytes: u64,
        floors: &'a QualityFloors,
        cancel: CancellationToken,
        progress: ProgressSender,
        sidecars: &'a SidecarPaths,
        temp_dir: &'a Path,
    ) -> Self {
        JobContext {
            input,
            output,
            target_bytes,
            floors,
            cancel,
            progress,
            sidecars,
            temp_dir,
            policy: default_policy(),
            category: crate::detection::detect_category(&input.to_string_lossy()),
            declared: &Change::ALL,
            consent_gate: None,
        }
    }

    pub fn with_policy(mut self, policy: &'a Policy) -> Self {
        self.policy = policy;
        self
    }

    pub fn with_category(mut self, category: Category) -> Self {
        self.category = category;
        self
    }

    pub fn with_declared(mut self, declared: &'a [Change]) -> Self {
        self.declared = declared;
        self
    }

    pub fn with_consent_gate(mut self, gate: &'a dyn ConsentGate) -> Self {
        self.consent_gate = Some(gate);
        self
    }

    pub fn consent_for(&self, changes: &[Change]) -> ConsentSource {
        self.policy
            .consent(self.category, changes)
            .unwrap_or(ConsentSource::Default)
    }

    pub fn authorize(&self, changes: &[Change]) -> ConsentDecision {
        if let Some(gate) = self.consent_gate {
            return gate.authorize(self.category, changes);
        }
        if self.policy.check(self.category, changes) == Verdict::Allowed {
            return ConsentDecision::Allow(self.consent_for(changes));
        }
        ConsentDecision::Deny(
            self.policy
                .denial_reason(self.category, changes)
                .unwrap_or_else(|| "Needs permission".to_string()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::SidecarName;

    #[test]
    fn job_context_carries_the_resolved_sidecars_temp_and_progress() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.jpg");
        let output = dir.path().join("out.jpg");
        let temp = dir.path().join("tmp");
        std::fs::create_dir_all(&temp).unwrap();
        let floors = QualityFloors::default();
        let sidecars = SidecarPaths {
            vips: Some("/engines/vips".to_string()),
            ffmpeg: None,
            ffprobe: None,
            gs: None,
            qpdf: None,
            gs_resources: None,
        };
        let token = CancellationToken::new();
        let (progress, seen) = ProgressSender::test_collector();

        let ctx = JobContext::new(
            &input,
            &output,
            42,
            &floors,
            token.clone(),
            progress.clone(),
            &sidecars,
            &temp,
        );

        assert!(std::ptr::eq(ctx.input, input.as_path()));
        assert!(std::ptr::eq(ctx.output, output.as_path()));
        assert!(std::ptr::eq(ctx.floors, &floors));
        assert!(std::ptr::eq(ctx.sidecars, &sidecars));
        assert!(std::ptr::eq(ctx.temp_dir, temp.as_path()));
        assert_eq!(ctx.target_bytes, 42);
        assert_eq!(ctx.sidecars.require(SidecarName::Vips).unwrap(), "/engines/vips");

        token.cancel();
        assert!(ctx.cancel.is_cancelled());

        ctx.progress.send_progress(37);
        assert_eq!(*seen.lock().unwrap(), vec![37]);
    }
}
