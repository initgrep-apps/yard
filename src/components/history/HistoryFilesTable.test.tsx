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

import { beforeEach, describe, expect, it, vi } from "vitest"
import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import type { HistoryFile, HistorySession } from "@/types"

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }))

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }))
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

import { HistoryFilesTable } from "@/components/history/HistoryFilesTable"
import { invoke } from "@tauri-apps/api/core"

const invokeMockTyped = vi.mocked(invoke)
const KB = 1024

function file(overrides: Partial<HistoryFile> = {}): HistoryFile {
  return {
    name: "a.jpg",
    path: "/in/a.jpg",
    outputPath: "/out/a.jpg",
    category: "image",
    targetBytes: 200 * KB,
    originalSize: 1024,
    compressedSize: 512,
    status: "done",
    qualityUsed: 70,
    strategyUsed: "quality=70",
    error: null,
    durationMs: 2_100,
    completedAt: new Date(2026, 9, 25, 20, 2).getTime(),
    ...overrides,
  }
}


function session(files: HistoryFile[]): HistorySession {
  return {
    sessionId: "s1",
    startedAt: Date.now(),
    endedAt: Date.now() + 45_000,
    outputFolder: "/out",
    fileCount: files.length,
    files,
    outcome: {
      done: 0,
      bestEffort: 0,
      failed: 0,
      cancelled: 0,
      unsupported: 0,
      alreadyUnderTarget: 0,
      needsPermission: 0,
    },
    totalSavedBytes: 0,
    cancelled: false,
    interrupted: false,
    matchedFiles: null,
  }
}

function demoFiles(): HistoryFile[] {
  return [
    file({ name: "z-done.jpg", path: "/in/z-done.jpg", originalSize: 1_000, compressedSize: 800 }),
    file({
      name: "a-failed.pdf",
      path: "/in/a-failed.pdf",
      category: "pdf",
      status: "failed",
      outputPath: null,
      compressedSize: null,
      originalSize: 2_000,
      strategyUsed: null,
      qualityUsed: null,
      error: "locked",
    }),
    file({
      name: "m-best.png",
      path: "/in/m-best.png",
      status: "best_effort",
      originalSize: 1_800 * KB,
      compressedSize: 259 * KB,
      strategyUsed: "palette",
      qualityUsed: null,
    }),
    file({
      name: "b-under.pdf",
      path: "/in/b-under.pdf",
      category: "pdf",
      status: "already_under_target",
      originalSize: 950 * KB,
      compressedSize: 950 * KB,
      strategyUsed: null,
      qualityUsed: null,
    }),
    file({
      name: "c-video.mov",
      path: "/in/c-video.mov",
      category: "video",
      status: "done",
      originalSize: 4_000,
      compressedSize: 3_000,
      strategyUsed: "crf=24",
      qualityUsed: null,
    }),
    file({
      name: "d-doc.xlsx",
      path: "/in/d-doc.xlsx",
      category: "document",
      status: "done",
      originalSize: 5_000,
      compressedSize: 4_000,
      strategyUsed: "rezip",
      qualityUsed: null,
    }),
  ]
}

const fileNames = () =>
  screen.getAllByTestId("history-file-name").map((node) => node.textContent)

beforeEach(() => {
  vi.clearAllMocks()
})

describe("HistoryFilesTable", () => {
  it("renders exactly File, Size, Status data columns; no Method or Saved", () => {
    render(<HistoryFilesTable session={session(demoFiles())} />)

    expect(screen.getByRole("columnheader", { name: "File" })).toBeInTheDocument()
    expect(screen.getByRole("columnheader", { name: "Size" })).toBeInTheDocument()
    expect(screen.getByRole("columnheader", { name: "Status" })).toBeInTheDocument()
    expect(screen.queryByRole("columnheader", { name: "Method" })).not.toBeInTheDocument()
    expect(screen.queryByRole("columnheader", { name: "Saved" })).not.toBeInTheDocument()
  })

  it("renders the new size only, with an em dash for failed rows", () => {
    render(<HistoryFilesTable session={session(demoFiles())} />)

    expect(screen.getByText("800 B")).toBeInTheDocument()
    expect(screen.getByText("259 KB")).toBeInTheDocument()
    expect(screen.getByText("950 KB")).toBeInTheDocument()
    expect(screen.queryByText(/→/)).not.toBeInTheDocument()
    expect(screen.getByText("locked")).toBeInTheDocument()
    expect(screen.getByText("—")).toBeInTheDocument()
  })

  it("filters by search, type and outcome and shows N of M", async () => {
    const user = userEvent.setup()
    render(<HistoryFilesTable session={session(demoFiles())} />)

    expect(screen.getByTestId("history-table-count")).toHaveTextContent("6 of 6")

    await user.type(screen.getByRole("searchbox", { name: "Search files" }), "best")
    expect(screen.getByTestId("history-table-count")).toHaveTextContent("1 of 6")
    expect(fileNames()).toEqual(["m-best.png"])

    await user.click(screen.getByRole("button", { name: "Clear" }))
    expect(fileNames()).toHaveLength(6)

    await user.click(screen.getByRole("combobox", { name: "Filter by type" }))
    await user.click(await screen.findByRole("option", { name: "PDF" }))
    expect(screen.getByTestId("history-table-count")).toHaveTextContent("2 of 6")
    expect(fileNames()).toEqual(["a-failed.pdf", "b-under.pdf"])

    await user.click(screen.getByRole("button", { name: "Clear" }))
    await user.click(screen.getByRole("button", { name: /Needs attention/ }))
    expect(screen.getByTestId("history-table-count")).toHaveTextContent("2 of 6")
    expect(fileNames()).toEqual(["a-failed.pdf", "m-best.png"])

    await user.click(screen.getByRole("button", { name: /^Done/ }))
    expect(screen.getByTestId("history-table-count")).toHaveTextContent("3 of 6")

    await user.click(screen.getByRole("button", { name: "Clear" }))
    expect(screen.getByTestId("history-table-count")).toHaveTextContent("6 of 6")
  })

  it("clears filters and restores all rows", async () => {
    const user = userEvent.setup()
    render(<HistoryFilesTable session={session(demoFiles())} />)

    await user.type(screen.getByRole("searchbox", { name: "Search files" }), "nothing")
    expect(screen.getByText("No files match these filters.")).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Clear filters" }))

    expect(screen.getByTestId("history-table-count")).toHaveTextContent("6 of 6")
    expect(fileNames()).toHaveLength(6)
  })

  it("sorts File, Size and Status and exposes aria-sort", async () => {
    const user = userEvent.setup()
    render(<HistoryFilesTable session={session(demoFiles())} />)

    const fileSort = screen.getByRole("button", { name: "Sort by File" })
    const fileHeader = fileSort.closest("th")!
    expect(fileHeader).toHaveAttribute("aria-sort", "none")

    await user.click(fileSort)
    expect(fileHeader).toHaveAttribute("aria-sort", "ascending")
    expect(fileNames()).toEqual([
      "a-failed.pdf",
      "b-under.pdf",
      "c-video.mov",
      "d-doc.xlsx",
      "m-best.png",
      "z-done.jpg",
    ])

    await user.click(fileSort)
    expect(fileHeader).toHaveAttribute("aria-sort", "descending")
    expect(fileNames()[0]).toBe("z-done.jpg")

    await user.click(fileSort)
    expect(fileHeader).toHaveAttribute("aria-sort", "none")
    expect(fileNames()[0]).toBe("z-done.jpg")

    const sizeSort = screen.getByRole("button", { name: "Sort by Size" })
    await user.click(sizeSort)
    expect(fileNames()[0]).toBe("z-done.jpg")
    expect(fileNames()[fileNames().length - 1]).toBe("a-failed.pdf")

    const statusSort = screen.getByRole("button", { name: "Sort by Status" })
    await user.click(statusSort)
    expect(fileNames()[0]).toBe("a-failed.pdf")
    expect(fileNames()[1]).toBe("m-best.png")
  })

  it("expands one row at a time and renders the three-card detail", async () => {
    const user = userEvent.setup()
    render(<HistoryFilesTable session={session(demoFiles())} />)

    await user.click(screen.getByRole("button", { name: "Expand m-best.png" }))
    expect(screen.getByTestId("history-file-detail")).toBeInTheDocument()
    expect(screen.getByText("PNG palette · Lossy")).toBeInTheDocument()
    expect(screen.getByText("1.8 MB")).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Expand z-done.jpg" }))
    expect(screen.getAllByTestId("history-file-detail")).toHaveLength(1)
    expect(screen.queryByText("PNG palette · Lossy")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Collapse z-done.jpg" })).toHaveAttribute(
      "aria-expanded",
      "true"
    )
  })

  it("closes the detail when its row is filtered out", async () => {
    const user = userEvent.setup()
    render(<HistoryFilesTable session={session(demoFiles())} />)

    await user.click(screen.getByRole("button", { name: "Expand m-best.png" }))
    expect(screen.getByTestId("history-file-detail")).toBeInTheDocument()

    await user.type(screen.getByRole("searchbox", { name: "Search files" }), "zzz")
    expect(screen.queryByTestId("history-file-detail")).not.toBeInTheDocument()
  })

  it("shows the empty filtered row with a clear action", async () => {
    const user = userEvent.setup()
    render(<HistoryFilesTable session={session(demoFiles())} />)

    await user.type(screen.getByRole("searchbox", { name: "Search files" }), "zzz")

    expect(screen.getByText("No files match these filters.")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Clear filters" })).toBeInTheDocument()
  })

  it("keeps failed reveal in the row menu and disables compressed in the detail footer", async () => {
    const user = userEvent.setup()
    invokeMockTyped.mockResolvedValue(undefined)
    render(<HistoryFilesTable session={session(demoFiles())} />)

    const rows = screen.getAllByRole("button", { name: "File actions" })
    await user.click(rows[1])
    const menu = await screen.findByRole("menu")
    expect(within(menu).getByRole("menuitem", { name: "Show compressed" })).toHaveAttribute(
      "aria-disabled"
    )
    await user.click(within(menu).getByRole("menuitem", { name: "Show original" }))
    await waitFor(() =>
      expect(invokeMockTyped).toHaveBeenCalledWith("reveal_in_folder", {
        filePath: "/in/a-failed.pdf",
      })
    )

    await user.click(screen.getByRole("button", { name: "Expand a-failed.pdf" }))
    expect(screen.getByRole("button", { name: "Show compressed" })).toBeDisabled()
  })

  it("uses a sticky header, vertical scroll and no horizontal scroll contract", () => {
    render(<HistoryFilesTable session={session(demoFiles())} />)

    const scroll = screen.getByTestId("history-table-scroll")
    expect(scroll.className).toContain("overflow-y-auto")
    expect(scroll.className).toContain("overflow-x-hidden")

    const table = screen.getByRole("table")
    expect(table.className).toContain("table-fixed")

    const header = screen.getByRole("columnheader", { name: "File" })
    expect(header.className).toContain("sticky")
    expect(
      screen.queryByText("Expand a row for saved data, target, method and timing.")
    ).not.toBeInTheDocument()
  })

  it("uses the History muted tone for Under target badges", () => {
    render(<HistoryFilesTable session={session(demoFiles())} />)

    const badge = screen
      .getAllByText("Under target")
      .map((node) => node.closest('[data-slot="badge"]'))
      .find((node) => node !== null)!
    expect(badge.className).toContain("text-muted-foreground")
    expect(badge.className).not.toContain("text-primary")
  })

  it("does not virtualize 200 or fewer rows", () => {
    const files = Array.from({ length: 200 }, (_, index) =>
      file({ name: `file-${index}.png`, path: `/in/file-${index}.png` })
    )
    render(<HistoryFilesTable session={session(files)} />)

    expect(screen.getByTestId("history-table-scroll")).toHaveAttribute(
      "data-virtualized",
      "false"
    )
    expect(fileNames()).toHaveLength(200)
  })

  it("virtualizes above 200 rows", () => {
    const files = Array.from({ length: 201 }, (_, index) =>
      file({ name: `file-${index}.png`, path: `/in/file-${index}.png` })
    )
    render(<HistoryFilesTable session={session(files)} />)

    expect(screen.getByTestId("history-table-scroll")).toHaveAttribute(
      "data-virtualized",
      "true"
    )
  })

  it("closes an expanded detail when the keyed session changes", async () => {
    const user = userEvent.setup()
    const { rerender } = render(<HistoryFilesTable key="s1" session={session(demoFiles())} />)
    await user.click(screen.getByRole("button", { name: "Expand m-best.png" }))
    expect(screen.getByTestId("history-file-detail")).toBeInTheDocument()

    rerender(<HistoryFilesTable key="s2" session={session([file({ name: "new.jpg" })])} />)

    expect(screen.queryByTestId("history-file-detail")).not.toBeInTheDocument()
    expect(screen.getByText("new.jpg")).toBeInTheDocument()
  })
})
