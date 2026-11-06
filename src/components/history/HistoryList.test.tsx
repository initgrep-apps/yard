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
import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"

const { useHistoryMock } = vi.hoisted(() => ({
  useHistoryMock: vi.fn(),
}))

vi.mock("@/hooks/useHistory", () => ({ useHistory: useHistoryMock }))

import { HistoryList } from "@/components/history/HistoryList"
import {
  Sidebar,
  SidebarContent,
  SidebarProvider,
} from "@/components/ui/sidebar"
import {
  HistoryProvider,
  useHistoryContext,
} from "@/providers/HistoryProvider"
import {
  batchLabel,
  fileCountLabel,
  savedLabel,
} from "@/lib/historyFormat"
import type {
  HistoryFile,
  HistoryFilters,
  HistoryResponse,
  HistorySession,
} from "@/types"

function makeFile(
  n: number,
  originalSize: number,
  compressedSize: number | null,
): HistoryFile {
  return {
    name: `f${n}.jpg`,
    path: `/t/f${n}.jpg`,
    outputPath: null,
    category: "image",
    targetBytes: 512,
    originalSize,
    compressedSize,
    status: compressedSize === null ? "failed" : "done",
    qualityUsed: null,
    strategyUsed: null,
    error: null,
    durationMs: 100,
    completedAt: 0,
  }
}

function makeSession({
  id,
  startedAt,
  files = [],
  totalSavedBytes = 0,
  cancelled = false,
  interrupted = false,
  matchedFiles = null,
}: {
  id: string
  startedAt: number
  files?: HistoryFile[]
  totalSavedBytes?: number
  cancelled?: boolean
  interrupted?: boolean
  matchedFiles?: number | null
}): HistorySession {
  return {
    sessionId: id,
    startedAt,
    endedAt: startedAt + 1_000,
    outputFolder: "/out",
    fileCount: files.length,
    files,
    outcome: {
      done: files.length,
      bestEffort: 0,
      failed: 0,
      cancelled: 0,
      unsupported: 0,
      alreadyUnderTarget: 0,
      needsPermission: 0,
    },
    totalSavedBytes,
    cancelled,
    interrupted,
    matchedFiles,
  }
}

function makeResponse(
  sessions: HistorySession[],
  {
    page = 0,
    pageSize = 50,
    totalSessions = sessions.length,
  }: { page?: number; pageSize?: number; totalSessions?: number } = {},
): HistoryResponse {
  return {
    sessions,
    page,
    pageSize,
    totalSessions,
    totalFiles: 0,
    summary: { sessions: totalSessions, files: 0, savedBytes: 0, failedFiles: 0 },
    outputFolders: [],
    skippedLines: 0,
  }
}

function SelectionProbe() {
  const { selectedSessionId } = useHistoryContext()
  return (
    <span data-testid="selected-session">{selectedSessionId ?? "none"}</span>
  )
}

function renderList({
  sessions = [],
  page = 0,
  responsePage = page,
  pageSize = 50,
  totalSessions = sessions.length,
  loading = false,
  error = null,
  filters = {},
  response,
}: {
  sessions?: HistorySession[]
  page?: number
  responsePage?: number
  pageSize?: number
  totalSessions?: number
  loading?: boolean
  error?: string | null
  filters?: HistoryFilters
  response?: HistoryResponse | null
} = {}) {
  const setFilters = vi.fn()
  const setPage = vi.fn()
  const clearError = vi.fn()
  const refresh = vi.fn()
  const onOpenHistory = vi.fn()

  useHistoryMock.mockReturnValue({
    response:
      response === undefined
        ? makeResponse(sessions, {
            page: responsePage,
            pageSize,
            totalSessions,
          })
        : response,
    loading,
    error,
    filters,
    setFilters,
    page,
    setPage,
    refresh,
    clearHistory: vi.fn().mockResolvedValue(undefined),
    clearError,
  })

  render(
    <HistoryProvider>
      <SelectionProbe />
      <SidebarProvider>
        <Sidebar>
          <SidebarContent>
            <HistoryList onOpenHistory={onOpenHistory} />
          </SidebarContent>
        </Sidebar>
        <main />
      </SidebarProvider>
    </HistoryProvider>,
  )

  return { setFilters, setPage, clearError, refresh, onOpenHistory }
}

const scrollArea = () =>
  document.querySelector('[data-slot="scroll-area"]') as HTMLElement | null
const batchList = () => screen.getByLabelText("Batches")

beforeEach(() => {
  useHistoryMock.mockReset()
  window.innerWidth = 1024
})

describe("HistoryList", () => {
  it("pins the pagination below the only scroller", () => {
    renderList({ totalSessions: 120 })

    const area = scrollArea()
    expect(area).not.toBeNull()
    expect(area!.className).toContain("min-h-0")
    expect(area!.className).toContain("flex-1")
    expect(batchList()).toBeInTheDocument()
    expect(area).toContainElement(batchList())
    expect(area).not.toContainElement(screen.getByText(/Page \d+ of/))

    const root = area!.parentElement!
    const children = [...root.children]
    const pagination = screen.getByText(/Page \d+ of/).parentElement!

    expect(children.indexOf(area!)).toBeLessThan(children.indexOf(pagination))
  })

  it("does not render a history search field", () => {
    renderList()

    expect(screen.queryByRole("textbox", { name: /search/i })).toBeNull()
  })

  it("renders each batch with its label, meta line, and saved percent", () => {
    const startedAt = new Date().setHours(14, 31, 0, 0)
    const session = makeSession({
      id: "s1",
      startedAt,
      files: [makeFile(1, 1_000, 250)],
      totalSavedBytes: 750,
    })
    renderList({ sessions: [session] })

    expect(
      screen.getByText(batchLabel(startedAt, new Date())),
    ).toBeInTheDocument()
    expect(
      screen.getByText(
        `${fileCountLabel(1)} · ${savedLabel(750)} saved`,
      ),
    ).toBeInTheDocument()
    expect(screen.getByText("−75%")).toBeInTheDocument()
  })

  it("fills the sidebar width with each batch row", () => {
    renderList({
      sessions: [makeSession({ id: "s1", startedAt: Date.now() })],
    })

    const row = screen.getByTestId("batch-s1")
    expect(row.className).toContain("w-full")
    expect(row.className).toContain("px-2.5")
    expect(row.className).toContain("py-2")
    expect(row.getAttribute("data-slot")).toBe("item")
    expect(row.className).toContain("data-active:bg-sidebar-accent")
  })

  it("uses totalSavedBytes for an unfiltered session", () => {
    const session = makeSession({
      id: "s1",
      startedAt: Date.now(),
      files: [makeFile(1, 1_000, 250)],
      totalSavedBytes: 500,
    })
    renderList({ sessions: [session] })

    expect(screen.getByText("1 file · 500 B saved")).toBeInTheDocument()
    expect(screen.getByText("−50%")).toBeInTheDocument()
  })

  it("uses the matched file sum for a filtered session", () => {
    const session = makeSession({
      id: "s1",
      startedAt: Date.now(),
      files: [makeFile(1, 1_000, 250)],
      totalSavedBytes: 500,
      matchedFiles: 1,
    })
    renderList({ sessions: [session], filters: { statuses: ["done"] } })

    expect(screen.getByText("1 file · 750 B saved")).toBeInTheDocument()
    expect(screen.getByText("−75%")).toBeInTheDocument()
  })

  it("shows 0% for a failed-only batch instead of a full saving", () => {
    const startedAt = new Date().setHours(9, 12, 0, 0)
    renderList({
      sessions: [
        makeSession({
          id: "s1",
          startedAt,
          files: [makeFile(1, 1_000, null)],
        }),
      ],
    })

    expect(screen.getByText("0%")).toBeInTheDocument()
    expect(screen.queryByText("−100%")).toBeNull()
    expect(
      screen.getByText(`${fileCountLabel(1)} · 0 B saved`),
    ).toBeInTheDocument()
  })

  it("shows 0 B saved for a grown output", () => {
    renderList({
      sessions: [
        makeSession({
          id: "s1",
          startedAt: Date.now(),
          files: [makeFile(1, 1_000, 2_000)],
          totalSavedBytes: 0,
          matchedFiles: 1,
        }),
      ],
      filters: { statuses: ["done"] },
    })

    expect(screen.getByText("1 file · 0 B saved")).toBeInTheDocument()
    expect(screen.queryByText("− saved")).toBeNull()
  })

  it("shows cancelled instead of a percent for a cancelled batch", () => {
    renderList({
      sessions: [
        makeSession({
          id: "s1",
          startedAt: Date.now(),
          files: [makeFile(1, 1_000, null)],
          cancelled: true,
        }),
      ],
    })

    expect(screen.getByText("cancelled")).toBeInTheDocument()
    expect(screen.queryByText("0%")).toBeNull()
  })

  it("shows interrupted before cancelled or the percent", () => {
    renderList({
      sessions: [
        makeSession({
          id: "s1",
          startedAt: Date.now(),
          files: [makeFile(1, 1_000, 250)],
          totalSavedBytes: 750,
          interrupted: true,
          cancelled: true,
        }),
      ],
    })

    const label = screen.getByText("interrupted")
    expect(label).toBeInTheDocument()
    expect(label.className).toContain("text-destructive")
    expect(screen.queryByText("cancelled")).toBeNull()
    expect(screen.queryByText("−75%")).toBeNull()
  })

  it("opens History and selects the batch when a row is clicked", async () => {
    const user = userEvent.setup()
    const first = makeSession({ id: "s1", startedAt: Date.now() })
    const second = makeSession({ id: "s2", startedAt: Date.now() - 60_000 })
    const { onOpenHistory } = renderList({ sessions: [first, second] })

    const row = screen.getByTestId("batch-s2")
    await user.click(row)

    expect(screen.getByTestId("selected-session")).toHaveTextContent("s2")
    expect(row.hasAttribute("data-active")).toBe(true)
    expect(row.className).toContain("data-active:bg-sidebar-accent")
    expect(screen.getByTestId("batch-s1").hasAttribute("data-active")).toBe(
      false,
    )
    expect(onOpenHistory).toHaveBeenCalledTimes(1)
  })

  it("disables Previous on the first page and pages forward", async () => {
    const user = userEvent.setup()
    const { setPage } = renderList({ totalSessions: 120 })

    expect(screen.getByText("Page 1 of 3")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Previous page" })).toBeDisabled()

    const next = screen.getByRole("button", { name: "Next page" })
    expect(next).toBeEnabled()
    await user.click(next)
    expect(setPage).toHaveBeenCalledWith(1)
  })

  it("disables Next on the last page and pages back", async () => {
    const user = userEvent.setup()
    const { setPage } = renderList({ page: 2, totalSessions: 120 })

    expect(screen.getByText("Page 3 of 3")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Next page" })).toBeDisabled()

    const previous = screen.getByRole("button", { name: "Previous page" })
    expect(previous).toBeEnabled()
    await user.click(previous)
    expect(setPage).toHaveBeenCalledWith(1)
  })

  it("labels the page from the response after a failed page change", () => {
    renderList({
      page: 1,
      responsePage: 0,
      totalSessions: 120,
      sessions: [makeSession({ id: "s1", startedAt: Date.now() })],
    })

    expect(screen.getByText("Page 1 of 3")).toBeInTheDocument()
  })

  it("derives the page count from the response page size", () => {
    renderList({ totalSessions: 30, pageSize: 10 })

    expect(screen.getByText("Page 1 of 3")).toBeInTheDocument()
  })

  it("hides pagination when everything fits on one page", () => {
    renderList({ totalSessions: 5, pageSize: 50 })

    expect(screen.queryByText(/Page \d+ of/)).toBeNull()
    expect(screen.queryByRole("button", { name: "Previous page" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Next page" })).toBeNull()
  })

  it("hides pagination exactly when the totals equal the page size", () => {
    renderList({ totalSessions: 50, pageSize: 50 })

    expect(screen.queryByText(/Page \d+ of/)).toBeNull()
    expect(screen.queryByRole("button", { name: "Previous page" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Next page" })).toBeNull()
  })

  it("shows pagination as soon as the totals exceed the page size", () => {
    renderList({ totalSessions: 51, pageSize: 50 })

    expect(screen.getByText("Page 1 of 2")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Previous page" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Next page" })).toBeEnabled()
  })

  it("renders skeleton rows and hides the pager on the first load", () => {
    renderList({ response: null, loading: true })

    expect(batchList()).toHaveAttribute("aria-busy", "true")
    expect(screen.getAllByTestId("batch-skeleton")).toHaveLength(3)
    const skeletonRow = screen.getAllByTestId("batch-skeleton")[0].firstElementChild!
    expect(skeletonRow.className).toContain("px-2.5")
    expect(skeletonRow.className).toContain("py-2")
    expect(
      document.querySelectorAll('[data-slot="skeleton"]').length,
    ).toBeGreaterThan(0)
    expect(screen.queryByText(/Page \d+ of/)).toBeNull()
    expect(screen.queryByRole("button", { name: "Previous page" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Next page" })).toBeNull()
  })

  it("keeps the rows and dims them while refreshing", () => {
    const session = makeSession({ id: "s1", startedAt: Date.now() })
    renderList({ sessions: [session], totalSessions: 120, loading: true })

    expect(screen.getByTestId("batch-s1")).toBeInTheDocument()
    expect(document.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0)
    expect(batchList()).toHaveAttribute("aria-busy", "true")
    expect(batchList().className).toContain("opacity-60")
    expect(screen.getByRole("button", { name: "Previous page" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Next page" })).toBeDisabled()
  })

  it("clears aria-busy after loading", () => {
    renderList({ sessions: [makeSession({ id: "s1", startedAt: Date.now() })] })

    expect(batchList()).toHaveAttribute("aria-busy", "false")
    expect(batchList().className).not.toContain("opacity-60")
  })

  it("shows the empty state without pagination", () => {
    renderList()

    expect(screen.getByText("No batches yet.")).toBeInTheDocument()
    expect(screen.getByText("No batches yet.").className).toContain("px-2.5 py-2")
    expect(screen.queryByText(/Page \d+ of/)).toBeNull()
    expect(screen.queryByRole("button", { name: "Previous page" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Next page" })).toBeNull()
    expect(scrollArea()).not.toBeNull()
  })

  it("distinguishes an empty filtered result", () => {
    renderList({ filters: { statuses: ["failed"] } })

    expect(
      screen.getByText("No batches match these filters."),
    ).toBeInTheDocument()
    expect(screen.queryByText("No batches yet.")).toBeNull()
  })

  it("does not treat a sort-only filter as narrowing", () => {
    renderList({ filters: { sort: "oldest" } })

    expect(screen.getByText("No batches yet.")).toBeInTheDocument()
    expect(screen.queryByText("No batches match these filters.")).toBeNull()
  })

  it("keeps rows when filters are active", () => {
    renderList({
      sessions: [makeSession({ id: "s1", startedAt: Date.now() })],
      filters: { statuses: ["failed"] },
    })

    expect(screen.getByTestId("batch-s1")).toBeInTheDocument()
    expect(
      screen.queryByText("No batches match these filters."),
    ).toBeNull()
  })

  it("renders a muted alert with a Retry that clears the error and refetches", async () => {
    const user = userEvent.setup()
    const { clearError, refresh } = renderList({
      response: null,
      error: "Could not load history: boom",
    })

    const alert = screen.getByRole("alert")
    expect(alert).toHaveTextContent("Could not load history: boom")
    expect(alert.className).toContain("text-muted-foreground")
    expect(alert.className).toContain("px-2.5 py-2")
    expect(screen.queryByText(/Page \d+ of/)).toBeNull()
    expect(screen.queryByRole("button", { name: "Previous page" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Next page" })).toBeNull()

    await user.click(screen.getByRole("button", { name: "Retry" }))

    expect(clearError).toHaveBeenCalledTimes(1)
    expect(refresh).toHaveBeenCalledTimes(1)
    expect(clearError.mock.invocationCallOrder[0]).toBeLessThan(
      refresh.mock.invocationCallOrder[0],
    )
  })

  it("replaces stale rows with the alert and freezes the pager mid-page", () => {
    renderList({
      sessions: [makeSession({ id: "s1", startedAt: Date.now() })],
      page: 1,
      responsePage: 0,
      totalSessions: 120,
      error: "Could not load history: boom",
    })

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Could not load history: boom",
    )
    expect(screen.queryByTestId("batch-s1")).toBeNull()
    expect(screen.queryByText(/Page \d+ of/)).toBeNull()
    expect(screen.getByRole("button", { name: "Previous page" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Next page" })).toBeDisabled()
  })
})
