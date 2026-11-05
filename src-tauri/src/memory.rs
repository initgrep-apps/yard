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

use crate::settings::Performance;
use std::path::Path;
use std::sync::OnceLock;
use sysinfo::{Disk, Disks, System};

pub const BUDGET_FLOOR_BYTES: u64 = 300 * 1024 * 1024;
pub const BUDGET_CEILING_BYTES: u64 = 2 * 1024 * 1024 * 1024;

const MIB: u64 = 1024 * 1024;
const DOCUMENT_ESTIMATE_BYTES: u64 = 4 * MIB;
const VIDEO_BASE_MEMORY_BYTES: u64 = 400 * MIB;
const VIDEO_BASE_WIDTH: u32 = 1920;

static SYSTEM: OnceLock<System> = OnceLock::new();

fn system() -> &'static System {
    SYSTEM.get_or_init(|| {
        let mut sys = System::new();
        sys.refresh_memory();
        sys
    })
}

pub fn total_ram() -> u64 {
    system().total_memory()
}

pub fn process_rss() -> u64 {
    let pid = sysinfo::Pid::from(std::process::id() as usize);
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map(|p| p.memory()).unwrap_or(0)
}

pub fn derive_budget_from_total_ram(adaptive: bool, ceiling_mb: u32, total_ram: u64) -> u64 {
    if adaptive {
        (total_ram / 8).clamp(BUDGET_FLOOR_BYTES, BUDGET_CEILING_BYTES)
    } else {
        (u64::from(ceiling_mb) * MIB).clamp(BUDGET_FLOOR_BYTES, BUDGET_CEILING_BYTES)
    }
}

pub fn derive_budget(performance: &Performance) -> u64 {
    derive_budget_from_total_ram(
        performance.adaptive_memory,
        performance.memory_ceiling_mb,
        total_ram(),
    )
}

pub fn estimate_job(category: crate::models::Category, input_size: u64, width: Option<u32>) -> u64 {
    let estimate = match category {
        crate::models::Category::Image => input_size.saturating_mul(4),
        crate::models::Category::Pdf => input_size.saturating_mul(3),
        crate::models::Category::Document => DOCUMENT_ESTIMATE_BYTES,
        crate::models::Category::Video => {
            let width = width.unwrap_or(VIDEO_BASE_WIDTH);
            let scaled = (u128::from(VIDEO_BASE_MEMORY_BYTES) * u128::from(width))
                / u128::from(VIDEO_BASE_WIDTH);
            input_size.saturating_add(scaled as u64)
        }
        crate::models::Category::Other => 0,
    };
    eprintln!("info: memory estimate for {category:?}: {estimate} bytes");
    estimate
}

pub fn available_space(path: &Path) -> Option<u64> {
    let disks = Disks::new_with_refreshed_list();
    let mut best: Option<&Disk> = None;
    for disk in disks.list() {
        let mount = disk.mount_point();
        if path.starts_with(mount) {
            if best.map(|b| mount.as_os_str().len() > b.mount_point().as_os_str().len())
                .unwrap_or(true)
            {
                best = Some(disk);
            }
        }
    }
    best.map(|d| d.available_space())
}

pub fn check_disk_headroom(need_bytes: u64, available: Option<u64>) -> Result<(), String> {
    match available {
        Some(n) if n >= need_bytes => Ok(()),
        Some(n) => Err(format!(
            "Insufficient disk space: needed {need_bytes} bytes, available {n} bytes"
        )),
        None => Err(format!(
            "Could not determine available disk space for {need_bytes} bytes"
        )),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStats {
    pub total_ram_bytes: u64,
    pub budget_bytes: u64,
    pub process_rss_bytes: u64,
}

pub fn memory_stats(performance: &Performance) -> MemoryStats {
    MemoryStats {
        total_ram_bytes: total_ram(),
        budget_bytes: derive_budget(performance),
        process_rss_bytes: process_rss(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_clamps_between_300_mib_and_2_gib() {
        let table = [
            (1u64 << 30, BUDGET_FLOOR_BYTES),
            (4u64 << 30, 512 * MIB),
            (8u64 << 30, 1024 * MIB),
            (16u64 << 30, BUDGET_CEILING_BYTES),
            (32u64 << 30, BUDGET_CEILING_BYTES),
            (64u64 << 30, BUDGET_CEILING_BYTES),
        ];
        for (ram, expected) in table {
            let got = derive_budget_from_total_ram(true, 2048, ram);
            assert_eq!(got, expected, "ram={ram}");
        }
    }

    #[test]
    fn adaptive_off_uses_the_configured_ceiling() {
        assert_eq!(
            derive_budget_from_total_ram(false, 512, 64u64 << 30),
            512 * MIB
        );
        assert_eq!(
            derive_budget_from_total_ram(false, 4096, 64u64 << 30),
            BUDGET_CEILING_BYTES
        );
    }

    #[test]
    fn estimate_grows_with_input_size() {
        let small = estimate_job(crate::models::Category::Image, 1 * MIB, None);
        let big = estimate_job(crate::models::Category::Image, 1 * MIB * 1024, None);
        assert!(small < big);
    }

    #[test]
    fn document_estimate_is_bounded() {
        assert_eq!(
            estimate_job(crate::models::Category::Document, 100 * MIB, None),
            DOCUMENT_ESTIMATE_BYTES
        );
    }

    #[test]
    fn video_estimate_scales_by_width() {
        let base = estimate_job(crate::models::Category::Video, 0, Some(1920));
        let half = estimate_job(crate::models::Category::Video, 0, Some(960));
        assert_eq!(base, VIDEO_BASE_MEMORY_BYTES);
        assert_eq!(half, VIDEO_BASE_MEMORY_BYTES / 2);
    }

    #[test]
    fn check_disk_headroom_passes_when_room_exists() {
        assert!(check_disk_headroom(100, Some(200)).is_ok());
    }

    #[test]
    fn batch_fails_when_output_disk_is_short() {
        assert!(check_disk_headroom(200, Some(100)).is_err());
    }

    #[test]
    fn check_disk_headroom_fails_when_unknown() {
        assert!(check_disk_headroom(100, None).is_err());
    }
}
