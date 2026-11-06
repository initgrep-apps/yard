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

import type { Category, FileStatus, HistoryFile } from "@/types"
import { STATUS_TONES } from "@/lib/tokens"
import { strategyLabel } from "@/lib/strategyLabels"
import { formatSize } from "@/lib/sizeFormat"

export const HISTORY_STATUS_LABELS: Record<FileStatus, string> = {
  pending: "Pending",
  compressing: "Compressing",
  done: "Done",
  best_effort: "Best effort",
  failed: "Failed",
  cancelled: "Cancelled",
  unsupported: "Unsupported",
  already_under_target: "Under target",
  needs_permission: "Needs permission",
}

export const HISTORY_STATUS_ORDER: Record<FileStatus, number> = {
  failed: 0,
  needs_permission: 1,
  best_effort: 2,
  unsupported: 3,
  cancelled: 4,
  done: 5,
  already_under_target: 6,
  pending: 7,
  compressing: 8,
}

type Tone = (typeof STATUS_TONES)[FileStatus]

export const HISTORY_STATUS_TONES: Record<FileStatus, Tone> = {
  ...STATUS_TONES,
  already_under_target: STATUS_TONES.cancelled,
}

export type HistoryOutcomeFilter = "all" | "attention" | "done" | "under_target"
export type HistoryTypeFilter = Category | "all"

export interface BatchSizeSummary {
  originalBytes: number
  currentBytes: number
  savedBytes: number
  percent: number
}

export type FileSizeSummary = BatchSizeSummary

function summarize(originalBytes: number, currentBytes: number): BatchSizeSummary {
  const savedBytes = Math.max(0, originalBytes - currentBytes)
  return {
    originalBytes,
    currentBytes,
    savedBytes,
    percent:
      originalBytes > 0
        ? Math.min(100, Math.max(0, Math.round((savedBytes / originalBytes) * 100)))
        : 0,
  }
}

export function batchSizeSummary(files: HistoryFile[]): BatchSizeSummary {
  const originalBytes = files.reduce((sum, file) => sum + file.originalSize, 0)
  const currentBytes = files.reduce(
    (sum, file) => sum + (file.compressedSize ?? file.originalSize),
    0
  )
  return summarize(originalBytes, currentBytes)
}

export function fileSizeSummary(file: HistoryFile): FileSizeSummary {
  return summarize(file.originalSize, file.compressedSize ?? file.originalSize)
}

export function isNoOutputFile(file: HistoryFile): boolean {
  return (
    file.status === "failed" ||
    file.status === "unsupported" ||
    file.status === "cancelled" ||
    file.status === "needs_permission"
  )
}

export function currentSizeBytes(file: HistoryFile): number | null {
  if (isNoOutputFile(file)) return file.compressedSize
  return file.compressedSize ?? file.originalSize
}

export function currentSizeLabel(file: HistoryFile): string {
  const bytes = currentSizeBytes(file)
  return bytes === null ? "—" : formatSize(bytes)
}

export function historyMethodLabel(file: HistoryFile): string {
  if (file.status === "already_under_target") return "—"
  if (file.strategyUsed) {
    return strategyLabel(file.strategyUsed, file.category) || file.strategyUsed
  }
  if (file.qualityUsed !== null) {
    return strategyLabel(`quality=${file.qualityUsed}`, file.category) || `quality ${file.qualityUsed}`
  }
  return "—"
}

export function needsAttention(file: HistoryFile): boolean {
  return (
    file.status === "failed" ||
    file.status === "needs_permission" ||
    file.status === "best_effort" ||
    file.status === "unsupported" ||
    file.status === "cancelled"
  )
}

export function matchesOutcomeFilter(
  file: HistoryFile,
  filter: HistoryOutcomeFilter
): boolean {
  switch (filter) {
    case "all":
      return true
    case "attention":
      return needsAttention(file)
    case "done":
      return file.status === "done"
    case "under_target":
      return file.status === "already_under_target"
  }
}

export function filterHistoryFiles(
  files: HistoryFile[],
  filters: {
    search: string
    type: HistoryTypeFilter
    outcome: HistoryOutcomeFilter
  }
): HistoryFile[] {
  const search = filters.search.trim().toLowerCase()
  return files.filter((file) => {
    const matchesSearch = search.length === 0 || file.name.toLowerCase().includes(search)
    const matchesType = filters.type === "all" || file.category === filters.type
    return matchesSearch && matchesType && matchesOutcomeFilter(file, filters.outcome)
  })
}

export function historyOutcomeCounts(files: HistoryFile[]): {
  all: number
  attention: number
  done: number
  underTarget: number
} {
  return files.reduce(
    (counts, file) => {
      if (needsAttention(file)) counts.attention += 1
      if (file.status === "done") counts.done += 1
      if (file.status === "already_under_target") counts.underTarget += 1
      return counts
    },
    {
      all: files.length,
      attention: 0,
      done: 0,
      underTarget: 0,
    }
  )
}

export function outcomePillCounts(files: HistoryFile[]): {
  done: number
  underTarget: number
  bestEffort: number
  failed: number
  needsPermission: number
  other: number
} {
  return files.reduce(
    (counts, file) => {
      switch (file.status) {
        case "done":
          counts.done += 1
          break
        case "already_under_target":
          counts.underTarget += 1
          break
        case "best_effort":
          counts.bestEffort += 1
          break
        case "failed":
          counts.failed += 1
          break
        case "needs_permission":
          counts.needsPermission += 1
          break
        default:
          counts.other += 1
      }
      return counts
    },
    { done: 0, underTarget: 0, bestEffort: 0, failed: 0, needsPermission: 0, other: 0 }
  )
}

export function formatPreciseDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "—"
  if (ms < 60_000) {
    return `${(ms / 1_000).toFixed(1)} s`
  }
  if (ms < 3_600_000) {
    const minutes = Math.floor(ms / 60_000)
    const seconds = Math.floor((ms % 60_000) / 1_000)
    return `${minutes}m ${String(seconds).padStart(2, "0")}s`
  }
  const hours = Math.floor(ms / 3_600_000)
  const minutes = Math.floor((ms % 3_600_000) / 60_000)
  return `${hours}h ${String(minutes).padStart(2, "0")}m`
}

export function completedLabel(ms: number): string {
  const date = new Date(ms)
  if (!Number.isFinite(ms) || !Number.isFinite(date.getTime())) return "—"
  const day = new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
  }).format(date)
  const time = new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    minute: "2-digit",
  }).format(date)
  return `${day} · ${time}`
}

export function virtualItemKey(kind: "row" | "detail", path: string): string {
  return `${kind}:${path}`
}

export function shouldVirtualizeRows(count: number): boolean {
  return count > 200
}
