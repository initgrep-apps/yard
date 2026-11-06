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
import { render, screen } from "@testing-library/react"
import type { FileStatus, HistoryFile, HistorySession } from "@/types"
import { HistorySummaryCards } from "@/components/history/HistorySummaryCards"

const KB = 1024
const MB = 1024 * 1024
const ORIGINAL = Math.round(72.2 * MB)
const CURRENT = Math.round(13.7 * MB)
const OTHER_FILES = 12

function file(status: FileStatus, overrides: Partial<HistoryFile> = {}): HistoryFile {
  return {
    name: "file.png",
    path: "/in/file.png",
    outputPath: "/out/file.png",
    category: "image",
    targetBytes: 200 * KB,
    originalSize: KB,
    compressedSize: status === "failed" ? null : KB,
    status,
    qualityUsed: null,
    strategyUsed: null,
    error: null,
    durationMs: 1_000,
    completedAt: new Date(2026, 9, 25, 20, 2).getTime(),
    changes: [],
    consentSource: null,
    ...overrides,
  }
}

function session13(): HistorySession {
  const files: HistoryFile[] = [
    file("done", {
      name: "huge.png",
      originalSize: ORIGINAL - OTHER_FILES * KB,
      compressedSize: CURRENT - OTHER_FILES * KB,
    }),
    ...Array.from({ length: 6 }, (_, index) => file("done", { name: `done-${index}.png` })),
    ...Array.from({ length: 4 }, (_, index) =>
      file("already_under_target", { name: `under-${index}.png` })
    ),
    file("best_effort", { name: "best.png" }),
    file("failed", { name: "failed.png" }),
  ]
  return {
    sessionId: "s13",
    startedAt: Date.now(),
    endedAt: Date.now() + 26_400,
    outputFolder: "/out",
    fileCount: files.length,
    files,
    outcome: {
      done: 7,
      bestEffort: 1,
      failed: 1,
      cancelled: 0,
      unsupported: 0,
      alreadyUnderTarget: 4,
      needsPermission: 0,
    },
    totalSavedBytes: ORIGINAL - CURRENT,
    cancelled: false,
    interrupted: false,
    matchedFiles: null,
  }
}

describe("HistorySummaryCards", () => {
  it("card 1 shows the linear scale, current right, saved below, and no Saved label", () => {
    render(<HistorySummaryCards session={session13()} />)

    expect(screen.getByTestId("summary-current")).toHaveTextContent("13.7 MB")
    expect(screen.getByText("72.2 MB")).toBeInTheDocument()
    expect(screen.getByText("81% saved")).toBeInTheDocument()
    expect(screen.getByText("58.5 MB saved")).toBeInTheDocument()
    expect(screen.getByText("Current")).toBeInTheDocument()
    expect(screen.queryByText("Saved")).not.toBeInTheDocument()
    expect(
      screen.getByRole("img", {
        name: "Current size 13.7 MB of 72.2 MB original; 81% saved",
      })
    ).toBeInTheDocument()
    expect(screen.getByTestId("summary-current-bar")).toHaveStyle({ width: "19%" })
  })

  it("Files card centers 13 and renders the 2x2 outcome pills", () => {
    render(<HistorySummaryCards session={session13()} />)

    expect(screen.getByTestId("summary-files")).toHaveTextContent("13")
    expect(screen.getByTestId("summary-pill-done")).toHaveTextContent("Done7")
    expect(screen.getByTestId("summary-pill-under")).toHaveTextContent("Under target4")
    expect(screen.getByTestId("summary-pill-best")).toHaveTextContent("Best1")
    expect(screen.getByTestId("summary-pill-failed")).toHaveTextContent("Failed1")
    expect(screen.queryByTestId("summary-pill-other")).not.toBeInTheDocument()
  })

  it("Best effort and Failed tiles show centered counts without 1 file", () => {
    render(<HistorySummaryCards session={session13()} />)

    expect(screen.getByTestId("summary-best-effort")).toHaveTextContent("1")
    expect(screen.getByTestId("summary-failed")).toHaveTextContent("1")
    expect(screen.queryByText("1 file")).not.toBeInTheDocument()
  })

  it("adds an Other pill when unsupported or cancelled files exist", () => {
    const session = session13()
    session.files = [
      ...session.files,
      file("unsupported", { name: "unsupported.bin" }),
      file("cancelled", { name: "cancelled.bin" }),
    ]
    render(<HistorySummaryCards session={session} />)

    expect(screen.getByTestId("summary-pill-other")).toHaveTextContent("Other2")
    expect(screen.getByTestId("summary-files")).toHaveTextContent("15")
  })

  it("uses the 860px container breakpoint and neutral card surfaces", () => {
    render(<HistorySummaryCards session={session13()} />)

    const grid = screen.getByTestId("history-summary-cards")
    expect(grid.className).toContain("grid-cols-2")
    expect(grid.className).toContain("@min-[860px]:grid-cols-[2.2fr_1.6fr_.9fr_.9fr]")

    for (const testId of [
      "summary-size",
      "summary-files",
      "summary-best-effort",
      "summary-failed",
    ]) {
      const card = screen.getByTestId(testId)
      expect(card.className).toContain("bg-muted/40")
      expect(card.className).toContain("rounded-[14px]")
      expect(card.className).not.toContain("bg-amber")
    }
  })

  it("hides the middle current label when current equals original", () => {
    const underOnly = session13()
    underOnly.files = [
      file("already_under_target", {
        name: "only.png",
        originalSize: 950 * KB,
        compressedSize: 950 * KB,
      }),
    ]
    render(<HistorySummaryCards session={underOnly} />)

    expect(screen.getByText("0% saved")).toBeInTheDocument()
    expect(screen.getAllByText("950 KB")).toHaveLength(1)
    expect(screen.getByTestId("summary-current")).toHaveTextContent("950 KB")
    expect(screen.getByTestId("summary-current-bar")).toHaveStyle({ width: "100%" })
  })
})
