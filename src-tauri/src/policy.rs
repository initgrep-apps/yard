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

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::models::Category;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    LossyEncode,
    Downscale,
    DropStream,
    FormatChange,
    MetadataStrip,
    Rasterize,
    PageDrop,
}

impl Change {
    pub const ALL: [Change; 7] = [
        Change::LossyEncode,
        Change::Downscale,
        Change::DropStream,
        Change::FormatChange,
        Change::MetadataStrip,
        Change::Rasterize,
        Change::PageDrop,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Change::LossyEncode => "lossy_encode",
            Change::Downscale => "downscale",
            Change::DropStream => "drop_stream",
            Change::FormatChange => "format_change",
            Change::MetadataStrip => "metadata_strip",
            Change::Rasterize => "rasterize",
            Change::PageDrop => "page_drop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    NeedsConsent,
    Denied,
}

const MATRIX: [[Verdict; 5]; 7] = [
    // Image, Video, Pdf, Document, Other
    [
        Verdict::Allowed,
        Verdict::Allowed,
        Verdict::Allowed,
        Verdict::Allowed,
        Verdict::Denied,
    ],
    [
        Verdict::NeedsConsent,
        Verdict::NeedsConsent,
        Verdict::Denied,
        Verdict::Denied,
        Verdict::Denied,
    ],
    [
        Verdict::Denied,
        Verdict::NeedsConsent,
        Verdict::Denied,
        Verdict::Denied,
        Verdict::Denied,
    ],
    [
        Verdict::NeedsConsent,
        Verdict::NeedsConsent,
        Verdict::Denied,
        Verdict::Denied,
        Verdict::Denied,
    ],
    [
        Verdict::Allowed,
        Verdict::Allowed,
        Verdict::Allowed,
        Verdict::Allowed,
        Verdict::Denied,
    ],
    [
        Verdict::Denied,
        Verdict::Denied,
        Verdict::NeedsConsent,
        Verdict::Denied,
        Verdict::Denied,
    ],
    [
        Verdict::Denied,
        Verdict::Denied,
        Verdict::Denied,
        Verdict::Denied,
        Verdict::Denied,
    ],
];

fn column(category: Category) -> usize {
    match category {
        Category::Image => 0,
        Category::Video => 1,
        Category::Pdf => 2,
        Category::Document => 3,
        Category::Other => 4,
    }
}

fn row(change: Change) -> usize {
    match change {
        Change::LossyEncode => 0,
        Change::Downscale => 1,
        Change::DropStream => 2,
        Change::FormatChange => 3,
        Change::MetadataStrip => 4,
        Change::Rasterize => 5,
        Change::PageDrop => 6,
    }
}

pub fn verdict(category: Category, change: Change) -> Verdict {
    MATRIX[row(change)][column(category)]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsentSource {
    Default,
    Global,
    Category,
    Batch,
    Once,
}

impl ConsentSource {
    fn rank(self) -> u8 {
        match self {
            ConsentSource::Default => 0,
            ConsentSource::Global => 1,
            ConsentSource::Category => 2,
            ConsentSource::Batch => 3,
            ConsentSource::Once => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentSettings {
    pub allow_destructive: bool,
    pub heic_to_jpg: bool,
}

impl Default for ConsentSettings {
    fn default() -> Self {
        ConsentSettings {
            allow_destructive: false,
            heic_to_jpg: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Policy {
    settings: ConsentSettings,
    category_grants: HashSet<(Category, Change)>,
    batch_grants: HashSet<(Category, Change)>,
    once_grants: HashSet<(Category, Change)>,
}

impl Policy {
    pub fn new(settings: ConsentSettings) -> Self {
        Policy {
            settings,
            ..Policy::default()
        }
    }

    pub fn with_category_grant(mut self, category: Category, changes: &[Change]) -> Self {
        self.grant_category(category, changes);
        self
    }

    pub fn with_batch_grant(mut self, category: Category, changes: &[Change]) -> Self {
        self.grant_batch(category, changes);
        self
    }

    pub fn grant_category(&mut self, category: Category, changes: &[Change]) {
        self.category_grants
            .extend(changes.iter().map(|change| (category, *change)));
    }

    pub fn grant_batch(&mut self, category: Category, changes: &[Change]) {
        self.batch_grants
            .extend(changes.iter().map(|change| (category, *change)));
    }

    pub fn grant_once(&mut self, category: Category, changes: &[Change]) {
        self.once_grants
            .extend(changes.iter().map(|change| (category, *change)));
    }

    pub fn check(&self, category: Category, changes: &[Change]) -> Verdict {
        let mut needs_consent = false;
        for change in changes {
            match verdict(category, *change) {
                Verdict::Denied => return Verdict::Denied,
                Verdict::NeedsConsent => {
                    if self.grant_source(category, *change).is_none() {
                        needs_consent = true;
                    }
                }
                Verdict::Allowed => {}
            }
        }
        if needs_consent {
            Verdict::NeedsConsent
        } else {
            Verdict::Allowed
        }
    }

    pub fn needs_consent(&self, category: Category, changes: &[Change]) -> Vec<Change> {
        changes
            .iter()
            .copied()
            .filter(|change| {
                verdict(category, *change) == Verdict::NeedsConsent
                    && self.grant_source(category, *change).is_none()
            })
            .collect()
    }

    pub fn consent(&self, category: Category, changes: &[Change]) -> Option<ConsentSource> {        if self.check(category, changes) != Verdict::Allowed {
            return None;
        }
        let mut source = ConsentSource::Default;
        for change in changes {
            if verdict(category, *change) == Verdict::NeedsConsent {
                let grant = self.grant_source(category, *change)?;
                if grant.rank() > source.rank() {
                    source = grant;
                }
            }
        }
        Some(source)
    }

    pub fn denial_reason(&self, category: Category, changes: &[Change]) -> Option<String> {
        if self.check(category, changes) == Verdict::Allowed {
            return None;
        }
        let mut blocked: Vec<&str> = Vec::new();
        for change in changes {
            let blocked_change = match verdict(category, *change) {
                Verdict::Denied => true,
                Verdict::NeedsConsent => self.grant_source(category, *change).is_none(),
                Verdict::Allowed => false,
            };
            if blocked_change {
                blocked.push(change.as_str());
            }
        }
        Some(format!("Needs permission: {}", blocked.join(", ")))
    }

    fn grant_source(&self, category: Category, change: Change) -> Option<ConsentSource> {
        let key = (category, change);
        if self.once_grants.contains(&key) {
            Some(ConsentSource::Once)
        } else if self.batch_grants.contains(&key) {
            Some(ConsentSource::Batch)
        } else if self.category_grants.contains(&key) {
            Some(ConsentSource::Category)
        } else if self.settings.allow_destructive {
            Some(ConsentSource::Global)
        } else if category == Category::Image
            && change == Change::FormatChange
            && self.settings.heic_to_jpg
        {
            Some(ConsentSource::Default)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{CompressionResult, Ladder};
    use crate::models::Category;
    use crate::queue::{decide_result_status, settle_result_status};
    use std::path::PathBuf;

    fn result(
        changes: Vec<Change>,
        consent: ConsentSource,
        met_target: bool,
        output: PathBuf,
    ) -> CompressionResult {
        CompressionResult {
            output_path: output,
            compressed_size: 100,
            quality_used: 70,
            met_target,
            strategy_used: "test".to_string(),
            changes,
            consent,
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        input: PathBuf,
        output: PathBuf,
        temp: PathBuf,
        floors: crate::settings::QualityFloors,
        sidecars: crate::engine::SidecarPaths,
        policy: Policy,
    }

    impl Fixture {
        fn new(policy: Policy) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("in.mp4");
            let output = dir.path().join("out.mp4");
            let temp = dir.path().join("tmp");
            std::fs::create_dir_all(&temp).unwrap();
            std::fs::write(&input, b"input").unwrap();
            Fixture {
                _dir: dir,
                input,
                output,
                temp,
                floors: crate::settings::QualityFloors::default(),
                sidecars: crate::engine::SidecarPaths {
                    vips: None,
                    ffmpeg: None,
                    ffprobe: None,
                    gs: None,
                    qpdf: None,
                    gs_resources: None,
                },
                policy,
            }
        }

        fn ctx<'b>(&'b self, declared: &'b [Change]) -> crate::engine::JobContext<'b> {
            crate::engine::JobContext::new(
                &self.input,
                &self.output,
                5,
                &self.floors,
                tokio_util::sync::CancellationToken::new(),
                crate::engine::ProgressSender::test_stub(),
                &self.sidecars,
                &self.temp,
            )
            .with_policy(&self.policy)
            .with_category(Category::Video)
            .with_declared(declared)
        }

        fn attempt(&self) -> PathBuf {
            self.temp.join("attempt-0.mp4")
        }
    }

    fn produced(path: PathBuf, size: u64, changes: Vec<Change>) -> crate::engine::AttemptOutcome {
        crate::engine::AttemptOutcome::Produced {
            path,
            size,
            quality: 1,
            strategy: "test".to_string(),
            changes,
            consent: ConsentSource::Default,
        }
    }

    #[test]
    fn matrix_matches_the_agreed_table() {
        use Category::{Document, Image, Other, Pdf, Video};
        use Change::{
            Downscale, DropStream, FormatChange, LossyEncode, MetadataStrip, PageDrop, Rasterize,
        };
        use Verdict::{Allowed, Denied, NeedsConsent};

        let agreed: [[Verdict; 5]; 7] = [
            [Allowed, Allowed, Allowed, Allowed, Denied],
            [NeedsConsent, NeedsConsent, Denied, Denied, Denied],
            [Denied, NeedsConsent, Denied, Denied, Denied],
            [NeedsConsent, NeedsConsent, Denied, Denied, Denied],
            [Allowed, Allowed, Allowed, Allowed, Denied],
            [Denied, Denied, NeedsConsent, Denied, Denied],
            [Denied, Denied, Denied, Denied, Denied],
        ];
        let rows = [
            LossyEncode,
            Downscale,
            DropStream,
            FormatChange,
            MetadataStrip,
            Rasterize,
            PageDrop,
        ];
        let columns = [Image, Video, Pdf, Document, Other];

        for (row, change) in rows.iter().enumerate() {
            for (column, category) in columns.iter().enumerate() {
                assert_eq!(
                    verdict(*category, *change),
                    agreed[row][column],
                    "matrix drifted at {change:?} / {category:?}"
                );
            }
        }
    }

    #[test]
    fn denied_change_returns_needs_permission_and_no_output() {
        let fixture = Fixture::new(Policy::default());
        let attempt = fixture.attempt();
        std::fs::write(&attempt, b"attempt").unwrap();
        let ctx = fixture.ctx(&[Change::PageDrop]);
        let mut ladder = Ladder::new();

        let decision = ladder.record(
            &ctx,
            produced(attempt.clone(), 1, vec![Change::PageDrop]),
        );

        assert!(matches!(decision, crate::engine::AttemptDecision::Continue));
        assert!(!attempt.exists(), "the denied attempt must be removed");
        let error = ladder
            .finish(&ctx)
            .expect_err("a policy denial must not deliver");
        assert!(
            matches!(error, crate::engine::CompressionError::NeedsPermission(_)),
            "got {error:?}"
        );
        assert!(
            error.to_string().contains("page_drop"),
            "the denial must explain itself: {error}"
        );
        assert!(!fixture.output.exists(), "a denied result must leave no output file");
    }

    #[test]
    #[cfg(unix)]
    fn denied_change_leaves_no_output_when_output_directory_is_read_only() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new(Policy::default());
        let attempt = fixture.attempt();
        std::fs::write(&attempt, b"attempt").unwrap();
        let output_dir = fixture._dir.path();
        let original = std::fs::metadata(output_dir).unwrap().permissions();
        std::fs::set_permissions(output_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let ctx = fixture.ctx(&[Change::PageDrop]);
        let mut ladder = Ladder::new();
        ladder.record(&ctx, produced(attempt, 1, vec![Change::PageDrop]));
        let error = ladder.finish(&ctx).expect_err("a policy denial must not deliver");

        assert!(matches!(
            error,
            crate::engine::CompressionError::NeedsPermission(_)
        ));
        assert!(
            !fixture.output.exists(),
            "a read-only output directory must never contain a denied delivery"
        );
        std::fs::set_permissions(output_dir, original).unwrap();
    }

    #[test]
    fn settle_guard_removes_a_delivered_file_that_should_not_have_been_delivered() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("out.pdf");
        std::fs::write(&output, b"must be removed").unwrap();
        let result = result(vec![Change::PageDrop], ConsentSource::Default, false, output.clone());

        let status = settle_result_status(&result, Category::Pdf, &Policy::default());

        assert_eq!(status, crate::models::FileStatus::NeedsPermission);
        assert!(!output.exists(), "the settle guard must remove a stale delivery");
    }

    #[test]
    fn granted_best_effort_delivers_after_the_policy_check() {
        let policy = Policy::new(ConsentSettings::default())
            .with_batch_grant(Category::Video, &[Change::Downscale]);
        let fixture = Fixture::new(policy);
        let attempt = fixture.attempt();
        std::fs::write(&attempt, b"attempt").unwrap();
        let ctx = fixture.ctx(&[Change::Downscale]);
        let mut ladder = Ladder::new();

        ladder.record(
            &ctx,
            crate::engine::AttemptOutcome::Produced {
                path: attempt,
                size: 10,
                quality: 1,
                strategy: "test".to_string(),
                changes: vec![Change::Downscale],
                consent: ConsentSource::Batch,
            },
        );
        let result = ladder
            .finish(&ctx)
            .expect("a granted over-target attempt must deliver best effort");

        assert!(!result.met_target);
        assert_eq!(result.changes, vec![Change::Downscale]);
        assert_eq!(result.consent, ConsentSource::Batch);
        assert!(fixture.output.exists());
    }

    #[test]
    fn batch_grant_overrides_a_needs_consent_verdict() {
        let mut policy = Policy::new(ConsentSettings::default());
        let changes = [Change::Downscale];

        assert_eq!(
            policy.check(Category::Video, &changes),
            Verdict::NeedsConsent
        );

        policy.grant_batch(Category::Video, &changes);
        assert_eq!(policy.check(Category::Video, &changes), Verdict::Allowed);
        assert_eq!(
            policy.consent(Category::Video, &changes),
            Some(ConsentSource::Batch)
        );
    }

    #[test]
    fn best_effort_requires_every_change_granted() {
        let result = result(
            vec![Change::Downscale],
            ConsentSource::Default,
            false,
            PathBuf::from("out.mp4"),
        );

        assert_eq!(
            decide_result_status(&result, Category::Video, &Policy::default()),
            crate::models::FileStatus::NeedsPermission
        );

        let granted = Policy::new(ConsentSettings::default())
            .with_batch_grant(Category::Video, &[Change::Downscale]);
        assert_eq!(
            decide_result_status(&result, Category::Video, &granted),
            crate::models::FileStatus::BestEffort
        );
    }

    #[test]
    fn declared_capability_mismatch_is_rejected() {
        let fixture = Fixture::new(Policy::default());
        let attempt = fixture.attempt();
        std::fs::write(&attempt, b"attempt").unwrap();
        let ctx = fixture.ctx(&[]);
        let mut ladder = Ladder::new();

        let decision =
            ladder.record(&ctx, produced(attempt.clone(), 10, vec![Change::DropStream]));
        assert!(matches!(decision, crate::engine::AttemptDecision::Continue));

        let error = ladder
            .finish(&ctx)
            .expect_err("an undeclared change must reject the attempt");
        assert!(
            error.to_string().contains("drop_stream"),
            "the rejection must name the undeclared change: {error}"
        );
        assert!(!attempt.exists(), "the rejected attempt must be removed");
        assert!(!fixture.output.exists(), "a rejected rung must not deliver");
        assert!(Ladder::check_declared(&[Change::DropStream], &[Change::DropStream]).is_ok());
    }

    #[test]
    fn default_heic_grant_is_allowed_and_reported_as_default() {
        let policy = Policy::new(ConsentSettings::default());
        let changes = [Change::FormatChange];

        assert_eq!(policy.check(Category::Image, &changes), Verdict::Allowed);
        assert_eq!(
            policy.consent(Category::Image, &changes),
            Some(ConsentSource::Default)
        );
        assert_eq!(
            policy.check(Category::Video, &changes),
            Verdict::NeedsConsent,
            "the HEIC default must not grant MOV to MP4"
        );
    }

    #[test]
    fn consent_sources_serialize_lowercase() {
        for (source, expected) in [
            (ConsentSource::Default, "\"default\""),
            (ConsentSource::Global, "\"global\""),
            (ConsentSource::Category, "\"category\""),
            (ConsentSource::Batch, "\"batch\""),
            (ConsentSource::Once, "\"once\""),
        ] {
            assert_eq!(serde_json::to_string(&source).unwrap(), expected);
        }
    }
}
