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

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const KB: u64 = 1024;
const MB: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryTarget {
    pub size: u64,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageQualityFloor {
    pub min_quality: u32,
}

impl ImageQualityFloor {
    fn spec_default() -> Self {
        ImageQualityFloor { min_quality: 30 }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoQualityFloor {
    pub min_crf: u32,
    pub min_resolution: String,
}

impl VideoQualityFloor {
    fn spec_default() -> Self {
        VideoQualityFloor {
            min_crf: 28,
            min_resolution: "480p".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfQualityFloor {
    pub min_dpi: u32,
    pub min_jpeg_quality: u32,
}

impl PdfQualityFloor {
    fn spec_default() -> Self {
        PdfQualityFloor {
            min_dpi: 72,
            min_jpeg_quality: 15,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentQualityFloor {
    pub min_media_quality: u32,
}

impl DocumentQualityFloor {
    fn spec_default() -> Self {
        DocumentQualityFloor {
            min_media_quality: 40,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Performance {
    pub adaptive_memory: bool,
    pub memory_ceiling_mb: u32,
}

impl Default for Performance {
    fn default() -> Self {
        Performance {
            adaptive_memory: true,
            memory_ceiling_mb: 2048,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySettings {
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
}

fn default_retention_days() -> u32 {
    90
}

impl Default for HistorySettings {
    fn default() -> Self {
        HistorySettings {
            retention_days: default_retention_days(),
        }
    }
}

impl HistorySettings {
    pub fn effective_retention_days(&self) -> u32 {
        let raw = self.retention_days;
        let effective = if raw == 0 {
            default_retention_days()
        } else {
            raw.clamp(1, 3650)
        };
        if raw != effective {
            eprintln!("warn: invalid history.retentionDays {raw}, using {effective}");
        }
        effective
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityFloors {
    #[serde(default = "ImageQualityFloor::spec_default")]
    pub image: ImageQualityFloor,
    #[serde(default = "VideoQualityFloor::spec_default")]
    pub video: VideoQualityFloor,
    #[serde(default = "PdfQualityFloor::spec_default")]
    pub pdf: PdfQualityFloor,
    #[serde(default = "DocumentQualityFloor::spec_default")]
    pub document: DocumentQualityFloor,
}

impl Default for QualityFloors {
    fn default() -> Self {
        QualityFloors {
            image: ImageQualityFloor::spec_default(),
            video: VideoQualityFloor::spec_default(),
            pdf: PdfQualityFloor::spec_default(),
            document: DocumentQualityFloor::spec_default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_output_folder")]
    pub output_folder: String,
    #[serde(default = "default_category_targets")]
    pub category_targets: std::collections::HashMap<String, CategoryTarget>,
    #[serde(default)]
    pub quality_floors: QualityFloors,
    #[serde(default)]
    pub performance: Performance,
    #[serde(default)]
    pub history: HistorySettings,
}

fn default_category_targets() -> std::collections::HashMap<String, CategoryTarget> {
    let mut category_targets = std::collections::HashMap::new();
    category_targets.insert(
        "image".to_string(),
        CategoryTarget {
            size: 200,
            unit: "KB".to_string(),
        },
    );
    category_targets.insert(
        "video".to_string(),
        CategoryTarget {
            size: 10,
            unit: "MB".to_string(),
        },
    );
    category_targets.insert(
        "pdf".to_string(),
        CategoryTarget {
            size: 2,
            unit: "MB".to_string(),
        },
    );
    category_targets.insert(
        "document".to_string(),
        CategoryTarget {
            size: 1,
            unit: "MB".to_string(),
        },
    );
    category_targets
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            output_folder: default_output_folder(),
            category_targets: default_category_targets(),
            quality_floors: QualityFloors::default(),
            performance: Performance::default(),
            history: HistorySettings::default(),
        }
    }
}

fn default_output_folder() -> String {
    match dirs::document_dir() {
        Some(docs) => docs.join("Compressed").to_string_lossy().to_string(),
        None => "./Compressed".to_string(),
    }
}

impl Settings {
    pub fn target_bytes_for(&self, category: &str) -> u64 {
        match self.category_targets.get(category) {
            Some(target) => {
                let multiplier = match target.unit.as_str() {
                    "MB" => MB,
                    _ => KB,
                };
                target.size.saturating_mul(multiplier)
            }
            None => 0,
        }
    }
}

fn warn_legacy_max_concurrent() {
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!("warn: legacy performance.maxConcurrent ignored; use adaptiveMemory and memoryCeilingMb");
    });
}

pub fn parse_settings_value(value: Option<serde_json::Value>) -> Settings {
    if let Some(ref v) = value {
        if v.get("performance")
            .and_then(|p| p.get("maxConcurrent"))
            .is_some()
        {
            warn_legacy_max_concurrent();
        }
    }
    match value {
        Some(v) => serde_json::from_value(v).unwrap_or_else(|e| {
            eprintln!("warn: settings parse failed, using defaults: {e}");
            Settings::default()
        }),
        None => Settings::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let s = Settings::default();
        assert_eq!(s.category_targets.get("image").unwrap().size, 200);
        assert_eq!(s.category_targets.get("image").unwrap().unit, "KB");
        assert_eq!(s.category_targets.get("video").unwrap().size, 10);
        assert_eq!(s.category_targets.get("video").unwrap().unit, "MB");
        assert_eq!(s.category_targets.get("pdf").unwrap().size, 2);
        assert_eq!(s.category_targets.get("document").unwrap().size, 1);
        assert_eq!(s.quality_floors.image.min_quality, 30);
        assert_eq!(s.quality_floors.video.min_crf, 28);
        assert_eq!(s.quality_floors.video.min_resolution, "480p");
        assert_eq!(s.quality_floors.pdf.min_dpi, 72);
        assert_eq!(s.quality_floors.pdf.min_jpeg_quality, 15);
        assert_eq!(s.quality_floors.document.min_media_quality, 40);
    }

    #[test]
    fn test_target_bytes_conversion() {
        let s = Settings::default();
        assert_eq!(s.target_bytes_for("image"), 200 * 1024);
        assert_eq!(s.target_bytes_for("video"), 10 * 1024 * 1024);
        assert_eq!(s.target_bytes_for("pdf"), 2 * 1024 * 1024);
        assert_eq!(s.target_bytes_for("document"), 1 * 1024 * 1024);
    }

    #[test]
    fn test_settings_serde_roundtrip_camel_case() {
        let value = serde_json::to_value(Settings::default()).unwrap();
        assert!(value.get("outputFolder").is_some());
        assert!(value.get("categoryTargets").is_some());
        let floors = value.get("qualityFloors").unwrap();
        assert!(floors.get("image").unwrap().get("minQuality").is_some());
        assert!(floors.get("video").unwrap().get("minCrf").is_some());
        assert!(floors.get("pdf").unwrap().get("minJpegQuality").is_some());
        assert!(floors
            .get("document")
            .unwrap()
            .get("minMediaQuality")
            .is_some());
        let roundtripped: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(Settings::default(), roundtripped);
    }

    #[test]
    fn test_target_bytes_unknown_unit_falls_back_to_kb() {
        let mut s = Settings::default();
        s.category_targets.insert(
            "image".to_string(),
            CategoryTarget {
                size: 500,
                unit: "gb".to_string(),
            },
        );
        assert_eq!(s.target_bytes_for("image"), 500 * 1024);
    }

    #[test]
    fn test_target_bytes_for_missing_category_returns_zero() {
        let s = Settings::default();
        assert_eq!(s.target_bytes_for("other"), 0);
        assert_eq!(s.target_bytes_for(""), 0);
    }

    #[test]
    fn test_target_bytes_zero_size_returns_zero() {
        let mut s = Settings::default();
        s.category_targets.insert(
            "image".to_string(),
            CategoryTarget {
                size: 0,
                unit: "KB".to_string(),
            },
        );
        assert_eq!(s.target_bytes_for("image"), 0);
    }

    #[test]
    fn test_default_output_folder_ends_with_compressed() {
        let s = Settings::default();
        assert!(s.output_folder.ends_with("Compressed"));
    }

    #[test]
    fn test_parse_settings_value_corrupt_falls_back_to_defaults() {
        let corrupt = serde_json::json!({
            "outputFolder": "~/Documents/Compressed",
            "categoryTargets": { "image": { "size": "not-a-number", "unit": "KB" } }
        });
        assert_eq!(parse_settings_value(Some(corrupt)), Settings::default());
        assert_eq!(parse_settings_value(None), Settings::default());
    }

    #[test]
    fn test_parse_settings_value_partial_fills_missing_with_defaults() {
        let partial = serde_json::json!({
            "outputFolder": "/tmp/out"
        });
        let parsed = parse_settings_value(Some(partial));
        assert_eq!(parsed.output_folder, "/tmp/out");
        assert_eq!(parsed.category_targets.get("image").unwrap().size, 200);
        assert_eq!(parsed.quality_floors.pdf.min_dpi, 72);
    }

    #[test]
    fn performance_defaults_to_adaptive_2048() {
        let s = Settings::default();
        assert!(s.performance.adaptive_memory);
        assert_eq!(s.performance.memory_ceiling_mb, 2048);
    }

    #[test]
    fn performance_serde_roundtrip_camel_case() {
        let value = serde_json::to_value(Settings::default()).unwrap();
        let perf = value.get("performance").unwrap();
        assert!(perf.get("adaptiveMemory").is_some());
        assert!(perf.get("memoryCeilingMb").is_some());
        let roundtripped: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(Settings::default(), roundtripped);
    }

    #[test]
    fn parse_settings_value_missing_performance_fills_default() {
        let partial = serde_json::json!({ "outputFolder": "/tmp/out" });
        let parsed = parse_settings_value(Some(partial));
        assert!(parsed.performance.adaptive_memory);
        assert_eq!(parsed.performance.memory_ceiling_mb, 2048);
    }

    #[test]
    fn parse_settings_value_corrupt_performance_fills_default() {
        let corrupt = serde_json::json!({
            "outputFolder": "/x",
            "performance": {}
        });
        let parsed = parse_settings_value(Some(corrupt));
        assert_eq!(parsed.output_folder, "/x");
        assert_eq!(parsed.performance, Performance::default());
    }

    #[test]
    fn old_settings_with_max_concurrent_still_load() {
        let legacy = serde_json::json!({
            "outputFolder": "/custom/out",
            "categoryTargets": { "image": { "size": 300, "unit": "KB" } },
            "performance": { "maxConcurrent": "3" }
        });
        let parsed = parse_settings_value(Some(legacy));
        assert_eq!(parsed.output_folder, "/custom/out");
        assert_eq!(
            parsed.category_targets.get("image").unwrap().size,
            300,
            "unrelated fields preserved"
        );
        assert!(parsed.performance.adaptive_memory);
        assert_eq!(parsed.performance.memory_ceiling_mb, 2048);
    }

    #[test]
    fn history_retention_defaults_to_90() {
        assert_eq!(Settings::default().history.retention_days, 90);
    }

    #[test]
    fn history_serde_roundtrip_camel_case() {
        let value = serde_json::to_value(Settings::default()).unwrap();
        let history = value.get("history").unwrap();
        assert!(history.get("retentionDays").is_some());
        let roundtripped: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(Settings::default(), roundtripped);
    }

    #[test]
    fn history_retention_max_roundtrips() {
        let mut s = Settings::default();
        s.history.retention_days = 3650;
        let value = serde_json::to_value(&s).unwrap();
        let roundtripped: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(roundtripped.history.retention_days, 3650);
    }

    #[test]
    fn parse_settings_value_missing_history_fills_default() {
        let parsed = parse_settings_value(Some(serde_json::json!({ "outputFolder": "/tmp/out" })));
        assert_eq!(parsed.history.retention_days, 90);
        assert_eq!(parsed.output_folder, "/tmp/out", "other settings preserved");
    }

    #[test]
    fn parse_settings_value_empty_history_fills_default() {
        let parsed = parse_settings_value(Some(serde_json::json!({
            "outputFolder": "/tmp/out",
            "history": {}
        })));
        assert_eq!(parsed.history.retention_days, 90);
        assert_eq!(parsed.output_folder, "/tmp/out", "other settings preserved");
    }

    #[test]
    fn parse_settings_value_corrupt_history_fills_default() {
        let corrupt = serde_json::json!({
            "outputFolder": "/x",
            "history": { "retentionDays": "lots" }
        });
        assert_eq!(parse_settings_value(Some(corrupt)), Settings::default());
    }

    #[test]
    fn parse_settings_value_in_range_history_value_parses() {
        let parsed =
            parse_settings_value(Some(serde_json::json!({"history": {"retentionDays": 30}})));
        assert_eq!(parsed.history.retention_days, 30);
    }

    #[test]
    fn parse_settings_value_null_history_whole_settings_defaults() {
        let with_null = serde_json::json!({
            "outputFolder": "/tmp/out",
            "history": null
        });
        assert_eq!(parse_settings_value(Some(with_null)), Settings::default());
    }

    #[test]
    fn parse_history_effective_retention_integration_pins() {
        let parsed = parse_settings_value(Some(
            serde_json::json!({"history": {"retentionDays": 5000}}),
        ));
        assert_eq!(parsed.history.retention_days, 5000);
        assert_eq!(parsed.history.effective_retention_days(), 3650);

        let zero = parse_settings_value(Some(serde_json::json!({"history": {"retentionDays": 0}})));
        assert_eq!(zero.history.retention_days, 0);
        assert_eq!(zero.history.effective_retention_days(), 90);
    }

    #[test]
    fn effective_retention_days_clamps() {
        for (raw, want) in [
            (0u32, 90u32),
            (1, 1),
            (90, 90),
            (3649, 3649),
            (3650, 3650),
            (3651, 3650),
            (5000, 3650),
        ] {
            let mut s = Settings::default();
            s.history.retention_days = raw;
            assert_eq!(s.history.effective_retention_days(), want, "raw={raw}");
        }
    }

    #[test]
    fn test_parse_settings_value_empty_quality_floors_fills_defaults() {
        let partial = serde_json::json!({
            "outputFolder": "/tmp/out",
            "qualityFloors": {}
        });
        let parsed = parse_settings_value(Some(partial));
        assert_eq!(parsed.output_folder, "/tmp/out");
        assert_eq!(parsed.category_targets.get("image").unwrap().size, 200);
        assert_eq!(parsed.quality_floors.image.min_quality, 30);
        assert_eq!(parsed.quality_floors.video.min_crf, 28);
        assert_eq!(parsed.quality_floors.video.min_resolution, "480p");
        assert_eq!(parsed.quality_floors.pdf.min_dpi, 72);
        assert_eq!(parsed.quality_floors.pdf.min_jpeg_quality, 15);
        assert_eq!(parsed.quality_floors.document.min_media_quality, 40);
    }
}
