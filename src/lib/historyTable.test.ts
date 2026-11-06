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

import { describe, expect, it } from "vitest"
import type { FileStatus, HistoryFile } from "@/types"
import { STATUS_TONES } from "@/lib/tokens"
import {
  HISTORY_STATUS_LABELS,
  HISTORY_STATUS_ORDER,
  HISTORY_STATUS_TONES,
  batchSizeSummary,
  completedLabel,
  currentSizeBytes,
  currentSizeLabel,
  fileSizeSummary,
  filterHistoryFiles,
  formatPreciseDuration,
  historyMethodLabel,
  historyOutcomeCounts,
  virtualItemKey,
  matchesOutcomeFilter,
  needsAttention,
  outcomePillCounts,
  shouldVirtualizeRows,
} from "@/lib/historyTable"

const KB = 1024

function file(overrides: Partial<HistoryFile> = {}): HistoryFile {
  return {
    name: "a.png",
    path: "/in/a.png",
    outputPath: "/out/a.png",
    category: "image",
    targetBytes: 200 * KB,
    originalSize: 1_800 * KB,
    compressedSize: 259 * KB,
    status: "best_effort",
    qualityUsed: null,
    strategyUsed: "palette",
    error: null,
    durationMs: 2_100,
    completedAt: new Date(2026, 9, 25, 20, 2).getTime(),
    ...overrides,
  }
}

function withStatus(status: FileStatus, overrides: Partial<HistoryFile> = {}) {
  return file({ status, ...overrides })
}

describe("batchSizeSummary", () => {
  it("computes original/current/saved/percent from files", () => {
    const files = [
      file({ originalSize: 1_000, compressedSize: 200, status: "done" }),
      file({ originalSize: 2_000, compressedSize: 2_000, status: "already_under_target" }),
      file({ originalSize: 1_000, compressedSize: null, status: "failed" }),
    ]
    expect(batchSizeSummary(files)).toEqual({
      originalBytes: 4_000,
      currentBytes: 3_200,
      savedBytes: 800,
      percent: 20,
    })
  })

  it("never divides by zero", () => {
    expect(
      batchSizeSummary([file({ originalSize: 0, compressedSize: null })])
    ).toMatchObject({ percent: 0, savedBytes: 0 })
    expect(batchSizeSummary([])).toEqual({
      originalBytes: 0,
      currentBytes: 0,
      savedBytes: 0,
      percent: 0,
    })
  })

  it("keeps failed/cancelled/unsupported originals in the current total", () => {
    const files = [
      file({ status: "failed", originalSize: 500, compressedSize: null }),
      file({ status: "cancelled", originalSize: 500, compressedSize: null }),
      file({ status: "unsupported", originalSize: 500, compressedSize: null }),
      file({ status: "done", originalSize: 500, compressedSize: 100 }),
    ]
    expect(batchSizeSummary(files)).toEqual({
      originalBytes: 2_000,
      currentBytes: 1_600,
      savedBytes: 400,
      percent: 20,
    })
  })
})

describe("fileSizeSummary", () => {
  it("computes saved values for done, under-target, failed and best-effort", () => {
    expect(fileSizeSummary(file({ status: "done", originalSize: 1_000, compressedSize: 200 }))).toEqual({
      originalBytes: 1_000,
      currentBytes: 200,
      savedBytes: 800,
      percent: 80,
    })
    expect(
      fileSizeSummary(file({ status: "already_under_target", originalSize: 1_000, compressedSize: 1_000 }))
    ).toEqual({ originalBytes: 1_000, currentBytes: 1_000, savedBytes: 0, percent: 0 })
    expect(
      fileSizeSummary(file({ status: "failed", originalSize: 1_000, compressedSize: null }))
    ).toEqual({ originalBytes: 1_000, currentBytes: 1_000, savedBytes: 0, percent: 0 })
    expect(
      fileSizeSummary(file({ status: "best_effort", originalSize: 1_800 * KB, compressedSize: 259 * KB }))
    ).toEqual({
      originalBytes: 1_800 * KB,
      currentBytes: 259 * KB,
      savedBytes: 1_541 * KB,
      percent: 86,
    })
  })
})

describe("currentSizeBytes and currentSizeLabel", () => {
  it("uses the compressed size, the original for under-target, and null for no output", () => {
    expect(currentSizeBytes(file({ status: "done", compressedSize: 512 }))).toBe(512)
    expect(currentSizeBytes(file({ status: "already_under_target", compressedSize: null, originalSize: 950 }))).toBe(950)
    expect(currentSizeBytes(file({ status: "failed", compressedSize: null }))).toBeNull()
    expect(currentSizeLabel(file({ status: "failed", compressedSize: null }))).toBe("—")
    expect(currentSizeLabel(file({ status: "done", compressedSize: 512 }))).toBe("512 B")
  })
})

describe("historyMethodLabel", () => {
  it("humanizes strategies", () => {
    expect(historyMethodLabel(file({ strategyUsed: "palette" }))).toBe("PNG palette · Lossy")
    expect(historyMethodLabel(file({ strategyUsed: "crf=24" }))).toBe("H.264 CRF · Lossy")
    expect(historyMethodLabel(file({ strategyUsed: "rezip" }))).toBe("Rezip · Lossless")
  })

  it("uses the quality fallback for older entries", () => {
    expect(historyMethodLabel(file({ strategyUsed: null, qualityUsed: 70 }))).toBe("Re-encode · Lossy")
    expect(
      historyMethodLabel(
        file({ category: "document", strategyUsed: null, qualityUsed: 40 })
      )
    ).toBe("Rebuild · Lossless")
  })

  it("returns — for under-target and no-output rows, raw for unknown", () => {
    expect(historyMethodLabel(file({ status: "already_under_target", strategyUsed: null }))).toBe("—")
    expect(historyMethodLabel(file({ status: "failed", strategyUsed: null, qualityUsed: null }))).toBe("—")
    expect(historyMethodLabel(file({ strategyUsed: "mystery", qualityUsed: null }))).toBe("mystery")
  })
})

describe("status labels and tones", () => {
  it("uses Under target and the documented order", () => {
    expect(HISTORY_STATUS_LABELS.already_under_target).toBe("Under target")
    expect(HISTORY_STATUS_ORDER.failed).toBeLessThan(HISTORY_STATUS_ORDER.best_effort)
    expect(HISTORY_STATUS_ORDER.best_effort).toBeLessThan(HISTORY_STATUS_ORDER.unsupported)
    expect(HISTORY_STATUS_ORDER.unsupported).toBeLessThan(HISTORY_STATUS_ORDER.cancelled)
    expect(HISTORY_STATUS_ORDER.cancelled).toBeLessThan(HISTORY_STATUS_ORDER.done)
    expect(HISTORY_STATUS_ORDER.done).toBeLessThan(HISTORY_STATUS_ORDER.already_under_target)
    expect(HISTORY_STATUS_ORDER.already_under_target).toBeLessThan(HISTORY_STATUS_ORDER.pending)
    expect(HISTORY_STATUS_ORDER.pending).toBeLessThan(HISTORY_STATUS_ORDER.compressing)
  })

  it("maps under target to the muted history tone without changing STATUS_TONES", () => {
    expect(HISTORY_STATUS_TONES.already_under_target.text).toBe("text-muted-foreground")
    expect(STATUS_TONES.already_under_target.text).toBe("text-primary")
  })
})

describe("outcome filters and counts", () => {
  const files = [
    withStatus("done"),
    withStatus("done"),
    withStatus("already_under_target"),
    withStatus("best_effort"),
    withStatus("failed"),
    withStatus("cancelled"),
    withStatus("unsupported"),
  ]

  it("groups needs-attention outcomes", () => {
    expect(needsAttention(withStatus("failed"))).toBe(true)
    expect(needsAttention(withStatus("needs_permission"))).toBe(true)
    expect(needsAttention(withStatus("best_effort"))).toBe(true)
    expect(needsAttention(withStatus("cancelled"))).toBe(true)
    expect(needsAttention(withStatus("unsupported"))).toBe(true)
    expect(needsAttention(withStatus("done"))).toBe(false)
    expect(needsAttention(withStatus("already_under_target"))).toBe(false)
    expect(matchesOutcomeFilter(withStatus("done"), "all")).toBe(true)
    expect(matchesOutcomeFilter(withStatus("done"), "attention")).toBe(false)
    expect(matchesOutcomeFilter(withStatus("done"), "done")).toBe(true)
    expect(matchesOutcomeFilter(withStatus("already_under_target"), "under_target")).toBe(true)
  })

  it("counts session-wide outcome filters", () => {
    expect(historyOutcomeCounts(files)).toEqual({
      all: 7,
      attention: 4,
      done: 2,
      underTarget: 1,
    })
  })

  it("counts the four summary pills and Other", () => {
    expect(outcomePillCounts(files)).toEqual({
      done: 2,
      underTarget: 1,
      bestEffort: 1,
      failed: 1,
      needsPermission: 0,
      other: 2,
    })
  })
})

describe("filterHistoryFiles", () => {
  const files = [
    file({ name: "3d-layout.png", category: "image", status: "best_effort" }),
    file({ name: "contract.pdf", category: "pdf", status: "failed" }),
    file({ name: "IMG_5647.MOV", category: "video", status: "done" }),
    file({ name: "notes.txt", category: "document", status: "already_under_target" }),
  ]

  it("searches names case-insensitively", () => {
    expect(filterHistoryFiles(files, { search: "LAYOUT", type: "all", outcome: "all" })).toEqual([
      files[0],
    ])
    expect(filterHistoryFiles(files, { search: "  ", type: "all", outcome: "all" })).toEqual(files)
  })

  it("filters by category, outcome and both", () => {
    expect(filterHistoryFiles(files, { search: "", type: "pdf", outcome: "all" })).toEqual([
      files[1],
    ])
    expect(filterHistoryFiles(files, { search: "", type: "all", outcome: "attention" })).toEqual([
      files[0],
      files[1],
    ])
    expect(
      filterHistoryFiles(files, { search: "layout", type: "image", outcome: "attention" })
    ).toEqual([files[0]])
    expect(
      filterHistoryFiles(files, { search: "layout", type: "pdf", outcome: "all" })
    ).toEqual([])
  })
})

describe("formatPreciseDuration", () => {
  it("uses one decimal below a minute and the established shape above", () => {
    expect(formatPreciseDuration(2_100)).toBe("2.1 s")
    expect(formatPreciseDuration(26_400)).toBe("26.4 s")
    expect(formatPreciseDuration(999)).toBe("1.0 s")
    expect(formatPreciseDuration(102_000)).toBe("1m 42s")
    expect(formatPreciseDuration(7_500_000)).toBe("2h 05m")
  })

  it("returns — for invalid or non-positive values", () => {
    for (const value of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(formatPreciseDuration(value)).toBe("—")
    }
  })
})

describe("completedLabel", () => {
  it("uses an absolute month/day and 12-hour time", () => {
    expect(completedLabel(new Date(2026, 9, 25, 20, 2).getTime())).toBe("Oct 25 · 8:02 PM")
    expect(completedLabel(Number.NaN)).toBe("—")
  })
})

describe("virtualItemKey", () => {
  it("keys rows and details by path, never by index", () => {
    expect(virtualItemKey("row", "/in/a.png")).toBe("row:/in/a.png")
    expect(virtualItemKey("detail", "/in/a.png")).toBe("detail:/in/a.png")
    expect(
      new Set([
        virtualItemKey("row", "/in/a.png"),
        virtualItemKey("detail", "/in/a.png"),
        virtualItemKey("row", "/in/b.png"),
      ]).size
    ).toBe(3)
  })
})

describe("shouldVirtualizeRows", () => {
  it("only virtualizes above 200 rows", () => {
    expect(shouldVirtualizeRows(200)).toBe(false)
    expect(shouldVirtualizeRows(201)).toBe(true)
  })
})
