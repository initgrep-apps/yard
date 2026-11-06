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
import type {
  Category,
  FileStatus,
  HistoryFile,
  HistoryFilters,
  HistoryResponse,
  HistorySession,
} from "@/types"

const { useHistoryContextMock } = vi.hoisted(() => ({
  useHistoryContextMock: vi.fn(),
}))

vi.mock("@/providers/HistoryProvider", () => ({
  useHistoryContext: useHistoryContextMock,
}))
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }))
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

import { HistoryView } from "@/views/HistoryView"
import { invoke } from "@tauri-apps/api/core"
import { toast } from "sonner"
import { completedLabel, formatPreciseDuration } from "@/lib/historyTable"

const invokeMock = vi.mocked(invoke)
const MB = 1024 * 1024
const STARTED_AT = new Date(2026, 9, 25, 20, 2).getTime()

function doneFile(overrides: Partial<HistoryFile> = {}): HistoryFile {
  return {
    name: "a.jpg",
    path: "/in/a.jpg",
    outputPath: "/out/a.jpg",
    category: "image",
    targetBytes: 200 * 1024,
    originalSize: 2 * MB,
    compressedSize: 1 * MB,
    status: "done",
    qualityUsed: 70,
    strategyUsed: "quality=70",
    error: null,
    durationMs: 2_100,
    completedAt: STARTED_AT,
    ...overrides,
  }
}

function failedFile(overrides: Partial<HistoryFile> = {}): HistoryFile {
  return {
    name: "broken.pdf",
    path: "/in/broken.pdf",
    outputPath: null,
    category: "pdf",
    targetBytes: 2 * MB,
    originalSize: 2 * MB,
    compressedSize: null,
    status: "failed",
    qualityUsed: null,
    strategyUsed: null,
    error: "Cannot read file: locked",
    durationMs: 3,
    completedAt: STARTED_AT,
    ...overrides,
  }
}

function session(overrides: Partial<HistorySession> = {}): HistorySession {
  return {
    sessionId: "s1",
    startedAt: STARTED_AT,
    endedAt: STARTED_AT + 45_000,
    outputFolder: "/out",
    fileCount: 2,
    files: [doneFile(), failedFile()],
    outcome: {
      done: 1,
      bestEffort: 0,
      failed: 1,
      cancelled: 0,
      unsupported: 0,
      alreadyUnderTarget: 0,
      needsPermission: 0,
    },
    totalSavedBytes: 1 * MB,
    cancelled: false,
    interrupted: false,
    matchedFiles: null,
    ...overrides,
  }
}

function responseWith(sessions: HistorySession[]): HistoryResponse {
  return {
    sessions,
    page: 0,
    pageSize: 50,
    totalSessions: sessions.length,
    totalFiles: sessions.reduce((acc, item) => acc + item.files.length, 0),
    summary: {
      sessions: sessions.length,
      files: sessions.reduce((acc, item) => acc + item.files.length, 0),
      savedBytes: sessions.reduce((acc, item) => acc + item.totalSavedBytes, 0),
      failedFiles: sessions.reduce(
        (acc, item) => acc + item.files.filter((file) => file.status === "failed").length,
        0
      ),
    },
    outputFolders: [...new Set(sessions.map((item) => item.outputFolder))],
    skippedLines: 0,
  }
}

function renderView({
  sessions = [],
  selectedSessionId,
  loading = false,
  error = null,
  filters = {},
  response,
}: {
  sessions?: HistorySession[]
  selectedSessionId?: string | null
  loading?: boolean
  error?: string | null
  filters?: HistoryFilters
  response?: HistoryResponse | null
} = {}) {
  const setFilters = vi.fn()
  const setPage = vi.fn()
  const clearError = vi.fn()
  const refresh = vi.fn()
  const context = {
    response: response === undefined ? responseWith(sessions) : response,
    loading,
    error,
    filters,
    setFilters,
    page: 0,
    setPage,
    searchText: "",
    setSearchText: vi.fn(),
    selectedSessionId:
      selectedSessionId === undefined ? (sessions[0]?.sessionId ?? null) : selectedSessionId,
    selectSession: vi.fn(),
    expanded: true,
    setExpanded: vi.fn(),
    refresh,
    clearHistory: vi.fn().mockResolvedValue(undefined),
    clearError,
  }
  useHistoryContextMock.mockReturnValue(context)

  const view = render(<HistoryView />)
  const updateContext = (patch: Partial<typeof context>) => {
    Object.assign(context, patch)
    useHistoryContextMock.mockReturnValue({ ...context })
    view.rerender(<HistoryView />)
  }
  return {
    ...view,
    updateContext,
    setFilters,
    setPage,
    clearError,
    refresh,
  }
}

const attentionBadge = () =>
  screen
    .getAllByText("Needs attention")
    .map((node) => node.closest('[data-slot="badge"]'))
    .find((node) => node !== null) as HTMLElement | undefined

beforeEach(() => {
  vi.clearAllMocks()
  useHistoryContextMock.mockReset()
})

describe("HistoryView", () => {
  it("renders the absolute batch header, Completed in, attention badge, summary and reveal action", async () => {
    const user = userEvent.setup()
    invokeMock.mockResolvedValue(undefined)
    renderView({ sessions: [session()] })

    expect(screen.getByText(completedLabel(STARTED_AT))).toBeInTheDocument()
    expect(
      screen.getByText(`Completed in ${formatPreciseDuration(45_000)}`)
    ).toBeInTheDocument()
    expect(attentionBadge()).toBeInTheDocument()
    expect(screen.getByTestId("summary-current")).toHaveTextContent("3.0 MB")
    expect(screen.getByText("25% saved")).toBeInTheDocument()
    expect(screen.getByText("1.0 MB saved")).toBeInTheDocument()
    expect(screen.getByTestId("summary-files")).toHaveTextContent("2")
    expect(screen.queryByText(/2 files/)).not.toBeInTheDocument()
    expect(screen.queryByText(/Output:/)).toBeNull()

    await user.click(screen.getByRole("button", { name: "Reveal folder" }))
    expect(invokeMock).toHaveBeenCalledWith("open_folder", { path: "/out" })
  })

  it("does not wrap the view in an outer card and marks the content busy", () => {
    const { container, updateContext } = renderView({ sessions: [session()] })

    const content = screen.getByTestId("history-content")
    expect(content).toHaveAttribute("aria-busy", "false")
    expect(container.querySelector('[data-slot="card-content"]')).toBeNull()

    updateContext({ loading: true })

    expect(screen.getByTestId("history-content")).toHaveAttribute("aria-busy", "true")
    expect(screen.getByTestId("history-content").className).toContain("opacity-60")
    expect(screen.getByText("a.jpg")).toBeInTheDocument()
    expect(container.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0)
  })

  it("places the three-column table below the summary", () => {
    renderView({ sessions: [session()] })

    const summary = screen.getByTestId("history-summary-cards")
    const tableCard = screen.getByTestId("history-table-card")
    expect(
      summary.compareDocumentPosition(tableCard) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()

    expect(screen.getByRole("columnheader", { name: "File" })).toBeInTheDocument()
    expect(screen.getByRole("columnheader", { name: "Size" })).toBeInTheDocument()
    expect(screen.getByRole("columnheader", { name: "Status" })).toBeInTheDocument()
    expect(screen.queryByRole("columnheader", { name: "Method" })).not.toBeInTheDocument()
    expect(screen.queryByRole("columnheader", { name: "Saved" })).not.toBeInTheDocument()
    expect(screen.getByText("a.jpg")).toBeInTheDocument()
    expect(screen.getByText("broken.pdf")).toBeInTheDocument()
  })

  it("uses the loaded files for the summary and count of a filtered batch", () => {
    renderView({
      sessions: [
        session({
          fileCount: 9,
          matchedFiles: 2,
          totalSavedBytes: 999_999,
          files: [
            doneFile({ originalSize: 4096, compressedSize: 2048 }),
            doneFile({
              name: "b.jpg",
              path: "/in/b.jpg",
              outputPath: "/out/b.jpg",
              status: "best_effort",
              originalSize: 4096,
              compressedSize: 2048,
            }),
          ],
          outcome: {
            done: 5,
            bestEffort: 9,
            failed: 7,
            cancelled: 0,
            unsupported: 0,
            alreadyUnderTarget: 0,
            needsPermission: 0,
          },
        }),
      ],
    })

    expect(screen.getByTestId("summary-files")).toHaveTextContent("2")
    expect(screen.getByTestId("summary-current")).toHaveTextContent("4 KB")
    expect(screen.getByText("50% saved")).toBeInTheDocument()
    expect(screen.getByTestId("history-table-count")).toHaveTextContent("2 of 2")
    expect(screen.queryByText("2 / 9")).not.toBeInTheDocument()
  })

  it("keeps the no-batch, no-history, filtered-empty and corrupted states", async () => {
    const user = userEvent.setup()
    const noBatch = renderView({ sessions: [session()], selectedSessionId: null })
    expect(screen.getByText("No batch selected")).toBeInTheDocument()
    expect(screen.queryByTestId("history-summary-cards")).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Clear filters" })).not.toBeInTheDocument()
    noBatch.unmount()

    const noHistory = renderView({ sessions: [] })
    expect(screen.getByText("No history yet")).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Clear filters" })).not.toBeInTheDocument()
    noHistory.unmount()

    const { setFilters } = renderView({
      response: responseWith([]),
      filters: { statuses: ["failed"] },
    })
    expect(screen.getByText("No batches match these filters.")).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Clear filters" }))
    expect(setFilters).toHaveBeenCalledWith({
      search: undefined,
      statuses: undefined,
      categories: undefined,
      sinceMs: undefined,
      outputFolder: undefined,
      sort: undefined,
    })
  })

  it("shows the corruption empty state without a clear action", () => {
    renderView({ response: { ...responseWith([]), skippedLines: 3 } })

    expect(screen.getByText("History could not be read")).toBeInTheDocument()
    expect(screen.getByText("3 lines could not be read.")).toBeInTheDocument()
    expect(screen.getByText("Some history entries could not be read (3).")).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Clear filters" })).not.toBeInTheDocument()
  })

  it("shows skeleton rows while loading and the provider error banner", async () => {
    const user = userEvent.setup()
    const loading = renderView({ response: null, loading: true })
    expect(document.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(3)
    loading.unmount()

    const { clearError, refresh } = renderView({
      response: null,
      error: "Could not load history: disk error",
    })
    const banner = screen.getByRole("alert")
    expect(within(banner).getByText("Could not load history: disk error")).toBeInTheDocument()
    await user.click(within(banner).getByRole("button", { name: "Retry" }))
    expect(clearError).toHaveBeenCalledTimes(1)
    expect(refresh).toHaveBeenCalledTimes(1)
  })

  it("hides stale summary, files and reveal when an error follows a loaded response", () => {
    renderView({ sessions: [session()], error: "refresh failed" })

    expect(screen.getByRole("alert")).toHaveTextContent("refresh failed")
    expect(screen.queryByTestId("history-summary-cards")).not.toBeInTheDocument()
    expect(screen.queryByText("a.jpg")).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Reveal folder" })).not.toBeInTheDocument()
  })

  it("keeps the cancelled/interrupted messages and badges without a table", () => {
    const cancelled = renderView({
      sessions: [session({ files: [], fileCount: 0, cancelled: true })],
    })
    expect(screen.getByText("No files finished before this batch was cancelled.")).toBeInTheDocument()
    expect(screen.getByText("Cancelled")).toBeInTheDocument()
    expect(screen.queryByText("Interrupted")).not.toBeInTheDocument()
    expect(screen.queryByRole("columnheader", { name: "File" })).not.toBeInTheDocument()
    cancelled.unmount()

    const interrupted = renderView({
      sessions: [session({ files: [], fileCount: 0, interrupted: true })],
    })
    expect(
      screen.getByText("No files finished before this batch was interrupted.")
    ).toBeInTheDocument()
    expect(screen.getByText("Interrupted")).toBeInTheDocument()
    expect(screen.queryByText("Cancelled")).not.toBeInTheDocument()
    interrupted.unmount()

    renderView({ sessions: [session({ files: [], fileCount: 0 })] })
    expect(screen.getByText("No files in this batch.")).toBeInTheDocument()
    expect(screen.queryByRole("columnheader", { name: "File" })).not.toBeInTheDocument()
  })

  it("renders an em dash duration for an unfinished batch", () => {
    renderView({ sessions: [session({ endedAt: null })] })
    expect(screen.getByText("Completed in —")).toBeInTheDocument()
  })

  it("never reports 100% saved for a failed-only batch and uses an em dash for an empty aggregate", () => {
    const failed = renderView({
      sessions: [session({ fileCount: 1, files: [failedFile()], totalSavedBytes: 0 })],
    })
    expect(screen.getByTestId("summary-current")).toHaveTextContent("2.0 MB")
    expect(screen.getByText("0% saved")).toBeInTheDocument()
    expect(screen.queryByText("100% saved")).not.toBeInTheDocument()
    failed.unmount()

    renderView({ sessions: [session({ fileCount: 0, files: [], totalSavedBytes: 0 })] })
    expect(screen.getByTestId("summary-current")).toHaveTextContent("—")
    expect(screen.getByText("0 B saved")).toBeInTheDocument()
  })

  it("resets table state when the selected session changes", async () => {
    const user = userEvent.setup()
    const other = session({
      sessionId: "s2",
      startedAt: STARTED_AT + 60_000,
      endedAt: STARTED_AT + 61_000,
      fileCount: 1,
      files: [doneFile({ name: "b.jpg", path: "/in/b.jpg", outputPath: "/out/b.jpg" })],
      totalSavedBytes: 1 * MB,
    })
    const { updateContext } = renderView({
      sessions: [session(), other],
      selectedSessionId: "s1",
    })

    await user.type(screen.getByRole("searchbox", { name: "Search files" }), "a.jpg")
    expect(screen.getByTestId("history-table-count")).toHaveTextContent("1 of 2")

    updateContext({ selectedSessionId: "s2" })

    expect(screen.getByRole("searchbox", { name: "Search files" })).toHaveValue("")
    expect(screen.getByTestId("history-table-count")).toHaveTextContent("1 of 1")
    expect(screen.getByText("b.jpg")).toBeInTheDocument()
    expect(screen.queryByText("a.jpg")).not.toBeInTheDocument()
  })

  it("shows the skipped lines notice only when lines were skipped and there is no error", () => {
    const clean = renderView({ response: responseWith([session()]) })
    expect(screen.queryByText(/could not be read/)).not.toBeInTheDocument()
    clean.unmount()

    const skipped = renderView({
      response: { ...responseWith([session()]), skippedLines: 3 },
    })
    expect(screen.getByText("Some history entries could not be read (3).")).toBeInTheDocument()
    skipped.unmount()

    renderView({
      response: { ...responseWith([session()]), skippedLines: 3 },
      error: "boom",
    })
    expect(screen.queryByText(/could not be read/)).not.toBeInTheDocument()
  })

  it("does not throw for unknown runtime statuses or categories", () => {
    renderView({
      sessions: [
        session({
          fileCount: 1,
          files: [
            doneFile({
              status: "mystery" as FileStatus,
              category: "mystery" as Category,
            }),
          ],
        }),
      ],
    })

    expect(screen.getAllByText("mystery").length).toBeGreaterThan(0)
  })

  it("toasts a backend open-folder error exactly once without duplicating its prefix", async () => {
    const user = userEvent.setup()
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {})
    invokeMock.mockRejectedValue("Could not open folder /x: denied")
    renderView({ sessions: [session()] })

    await user.click(screen.getByRole("button", { name: "Reveal folder" }))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Could not open folder /x: denied")
    )
    expect(toast.error).toHaveBeenCalledTimes(1)

    errSpy.mockRestore()
  })
})
