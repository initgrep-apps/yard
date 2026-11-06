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

const { compressionMock, historyMock } = vi.hoisted(() => ({
  compressionMock: vi.fn(),
  historyMock: vi.fn(),
}))

vi.mock("@/providers/CompressionProvider", () => ({
  useCompressionContext: compressionMock,
}))
vi.mock("@/providers/HistoryProvider", () => ({
  useHistoryContext: historyMock,
}))

import { SidebarNav } from "@/components/shell/SidebarNav"
import { SidebarContent, SidebarProvider } from "@/components/ui/sidebar"
import { HISTORY_LIST_ID } from "@/components/history/HistoryList"
import type {
  FileEntry,
  HistoryFilters,
  HistoryResponse,
  HistorySession,
  ViewId,
} from "@/types"

const makeEntry = (n: number): FileEntry => ({
  id: `f${n}`,
  name: `f${n}.jpg`,
  path: `/t/f${n}.jpg`,
  category: "image",
  targetBytes: 204800,
  originalSize: 1024,
  status: "pending",
  compressedSize: null,
  outputPath: null,
  error: null,
  strategyUsed: null,
  qualityUsed: null,
})

const emptyHistory: HistoryResponse = {
  sessions: [],
  page: 0,
  pageSize: 50,
  totalSessions: 0,
  totalFiles: 0,
  summary: { sessions: 0, files: 0, savedBytes: 0, failedFiles: 0 },
  outputFolders: [],
  skippedLines: 0,
}

const makeSession = (id: string): HistorySession => ({
  sessionId: id,
  startedAt: Date.now(),
  endedAt: Date.now() + 1_000,
  outputFolder: "/out",
  fileCount: 0,
  files: [],
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
})

function renderNav({
  activeView = "compress",
  files = [],
  expanded = false,
  filters = {},
  sessions = [],
  totalSessions = sessions.length,
}: {
  activeView?: ViewId
  files?: FileEntry[]
  expanded?: boolean
  filters?: HistoryFilters
  sessions?: HistorySession[]
  totalSessions?: number
} = {}) {
  compressionMock.mockReturnValue({ files })
  const setExpanded = vi.fn()
  historyMock.mockReturnValue({
    response: { ...emptyHistory, sessions, totalSessions },
    loading: false,
    error: null,
    filters,
    setFilters: vi.fn(),
    page: 0,
    setPage: vi.fn(),
    searchText: "",
    setSearchText: vi.fn(),
    selectedSessionId: null,
    selectSession: vi.fn(),
    expanded,
    setExpanded,
    refresh: vi.fn(),
    clearHistory: vi.fn(),
    clearError: vi.fn(),
  })
  const onNavigate = vi.fn()
  const onOpenSettings = vi.fn()
  const onOpenFilter = vi.fn()

  render(
    <SidebarProvider>
      <SidebarContent>
        <SidebarNav
          activeView={activeView}
          onNavigate={onNavigate}
          onOpenSettings={onOpenSettings}
          onOpenFilter={onOpenFilter}
        />
      </SidebarContent>
    </SidebarProvider>,
  )

  return { onNavigate, onOpenSettings, onOpenFilter, setExpanded }
}

const content = () =>
  document.querySelector('[data-slot="sidebar-content"]') as HTMLElement
const scrollArea = () =>
  document.querySelector('[data-slot="scroll-area"]') as HTMLElement | null

beforeEach(() => {
  window.innerWidth = 1024
  compressionMock.mockReset()
  historyMock.mockReset()
})

describe("SidebarNav", () => {
  it("renders Compress and History, with Settings only in the footer", () => {
    renderNav()

    expect(
      screen.getByRole("button", { name: /Compress/ }),
    ).toBeInTheDocument()
    expect(screen.getByRole("button", { name: /History/ })).toBeInTheDocument()
    const settings = screen.getByTestId("nav-settings")
    expect(settings).toHaveTextContent("Settings")
    expect(settings.closest('[data-slot="sidebar-footer"]')).not.toBeNull()
  })

  it("opens Settings from the footer without navigating", async () => {
    const user = userEvent.setup()
    const { onNavigate, onOpenSettings } = renderNav()

    await user.click(screen.getByTestId("nav-settings"))

    expect(onOpenSettings).toHaveBeenCalledTimes(1)
    expect(onNavigate).not.toHaveBeenCalled()
  })

  it("toggles the batch list without navigating", async () => {
    const user = userEvent.setup()
    const { onNavigate, setExpanded } = renderNav()

    await user.click(screen.getByRole("button", { name: /History/ }))

    expect(setExpanded).toHaveBeenCalledWith(true)
    expect(onNavigate).not.toHaveBeenCalled()
  })

  it("collapses the expanded list when History is clicked again", async () => {
    const user = userEvent.setup()
    const { onNavigate, setExpanded } = renderNav({
      activeView: "history",
      expanded: true,
    })
    const row = screen.getByTestId("nav-history")
    expect(row).toHaveAttribute("aria-expanded", "true")

    await user.click(row)

    expect(setExpanded).toHaveBeenCalledWith(false)
    expect(onNavigate).not.toHaveBeenCalled()
  })

  it("navigates to Compress", async () => {
    const user = userEvent.setup()
    const { onNavigate } = renderNav({ activeView: "history" })

    await user.click(screen.getByRole("button", { name: /Compress/ }))

    expect(onNavigate).toHaveBeenCalledWith("compress")
  })

  it("marks the active row", () => {
    renderNav({ activeView: "history" })

    expect(screen.getByTestId("nav-history").getAttribute("data-slot")).toBe(
      "item",
    )
    expect(screen.getByTestId("nav-compress").hasAttribute("data-active")).toBe(
      false,
    )
    expect(screen.getByTestId("nav-compress")).not.toHaveAttribute(
      "aria-expanded",
    )
    expect(screen.getByTestId("nav-history").hasAttribute("data-active")).toBe(
      true,
    )
    expect(screen.getByTestId("nav-history")).toHaveAttribute(
      "aria-expanded",
      "false",
    )
  })

  it("renders an icon in every nav row", () => {
    renderNav()

    for (const id of ["nav-compress", "nav-history", "nav-settings"]) {
      const row = screen.getByTestId(id)
      expect(row.querySelector('[data-slot="item-media"] svg')).not.toBeNull()
    }
  })

  it("puts the chevron beside the label and the funnel at the far edge", () => {
    renderNav({ expanded: false })

    const row = screen.getByTestId("nav-history")
    expect(
      row.querySelector('[data-slot="item-title"] svg'),
    ).not.toBeNull()

    const funnel = screen.getByRole("button", { name: "Filter history" })
    expect(funnel.className).toContain("size-6!")
    expect(funnel.className).toContain("opacity-0")
    expect(funnel.className).toContain("pointer-events-none")
    expect(funnel.className).toContain("group-hover/menu-item:opacity-100")
    expect(funnel.className).toContain("focus-visible:opacity-100")
    expect(funnel.className).toContain("group-focus-within/menu-item:opacity-100")
    expect(funnel.className).toContain("right-2")
    expect(
      funnel.closest('[data-slot="sidebar-menu-item"]')!.className,
    ).toContain("group/menu-item")
  })

  it("keeps the funnel visible while filters are active", () => {
    renderNav({ filters: { categories: ["image"] } })

    const funnel = screen.getByRole("button", { name: "Filter history" })
    expect(funnel.className).not.toContain("opacity-0")
  })

  it("shows the staged file count on the Compress row", () => {
    renderNav({ files: [makeEntry(1), makeEntry(2), makeEntry(3)] })

    const badge = screen.getByTestId("nav-count")
    expect(badge).toHaveTextContent("3")
    expect(badge.className).toContain("top-1/2")
    expect(badge.className).toContain("-translate-y-1/2")
    expect(badge.className).toContain("right-2")
    expect(badge.closest('[data-slot="sidebar-menu-item"]')).not.toBeNull()
    expect(badge.closest('[data-slot="sidebar-menu-button"]')).toBeNull()
  })

  it("renders the history list only while expanded", () => {
    renderNav({ expanded: false })
    expect(document.getElementById(HISTORY_LIST_ID)).toBeNull()

    renderNav({ expanded: true })
    expect(document.getElementById(HISTORY_LIST_ID)).not.toBeNull()
  })

  it("links the History row to the batch list", () => {
    renderNav({ expanded: true })

    const row = screen.getByTestId("nav-history")
    expect(row.className).toContain("pr-9")
    expect(row).toHaveAttribute("aria-controls", HISTORY_LIST_ID)
    expect(row).toHaveAttribute("aria-expanded", "true")

    const list = document.getElementById(HISTORY_LIST_ID)
    expect(list).not.toBeNull()
    expect(list).toHaveAccessibleName("Batches")
    expect(list).not.toHaveAttribute("role")
  })

  it("opens the filter from the History row without navigating", async () => {
    const user = userEvent.setup()
    const { onNavigate, onOpenFilter } = renderNav()

    const funnel = screen.getByRole("button", { name: "Filter history" })
    expect(funnel).toHaveAttribute("title", "Filter")
    expect(funnel.className).toContain("size-6")

    await user.click(funnel)

    expect(onOpenFilter).toHaveBeenCalledTimes(1)
    expect(onNavigate).not.toHaveBeenCalled()
  })

  it("reveals the funnel from the keyboard and on hover", () => {
    renderNav({ expanded: false })

    const funnel = screen.getByRole("button", { name: "Filter history" })
    expect(funnel.className).toContain("focus-visible:pointer-events-auto")
    expect(funnel.className).toContain(
      "group-focus-within/menu-item:pointer-events-auto",
    )
  })

  it("rotates the chevron while collapsed", () => {
    renderNav({ expanded: false })
    const collapsed = screen
      .getByTestId("nav-history")
      .querySelector('[data-slot="item-title"] svg')!
    expect(collapsed.getAttribute("class")).toContain("-rotate-90")
  })

  it("draws the expanded chevron upright", () => {
    renderNav({ expanded: true })
    const expanded = screen
      .getByTestId("nav-history")
      .querySelector('[data-slot="item-title"] svg')!
    expect(expanded.getAttribute("class")).not.toContain("-rotate-90")
  })

  it("keeps the funnel visible on touch layouts", () => {
    window.innerWidth = 760
    renderNav({ expanded: false })

    const funnel = screen.getByRole("button", { name: "Filter history" })
    expect(funnel.className).not.toContain("opacity-0")
  })

  it("leaves the funnel unmarked by default", () => {
    renderNav()

    expect(screen.queryByTestId("history-filter-dot")).toBeNull()
  })

  it("leaves the funnel unmarked for newest-first", () => {
    renderNav({ filters: { sort: "newest" } })

    expect(screen.queryByTestId("history-filter-dot")).toBeNull()
  })

  it("leaves the funnel unmarked for oldest-first", () => {
    renderNav({ filters: { sort: "oldest" } })

    expect(screen.queryByTestId("history-filter-dot")).toBeNull()
  })

  it("marks the funnel for a status filter", () => {
    renderNav({ filters: { statuses: ["failed"] } })

    expect(screen.getByTestId("history-filter-dot")).toBeInTheDocument()
  })

  it("marks the funnel for a category filter", () => {
    renderNav({ filters: { categories: ["image"] } })

    expect(screen.getByTestId("history-filter-dot")).toBeInTheDocument()
  })

  it("marks the funnel for a since filter", () => {
    renderNav({ filters: { sinceMs: 1 } })

    expect(screen.getByTestId("history-filter-dot")).toBeInTheDocument()
  })

  it("marks the funnel for a search filter", () => {
    renderNav({ filters: { search: "hero" } })

    expect(screen.getByTestId("history-filter-dot")).toBeInTheDocument()
  })

  it("marks the funnel for an output folder filter", () => {
    renderNav({ filters: { outputFolder: "/out" } })

    expect(screen.getByTestId("history-filter-dot")).toBeInTheDocument()
  })

  it("omits aria-controls while the list is collapsed", () => {
    renderNav({ expanded: false })

    expect(screen.getByTestId("nav-history")).toHaveAttribute(
      "aria-expanded",
      "false",
    )
    expect(screen.getByTestId("nav-history")).not.toHaveAttribute(
      "aria-controls",
    )
  })

  it("navigates to History when a batch row is selected from Compress", async () => {
    const user = userEvent.setup()
    const session = makeSession("s1")
    const { onNavigate } = renderNav({
      activeView: "compress",
      expanded: true,
      sessions: [session],
    })

    await user.click(screen.getByTestId(`batch-${session.sessionId}`))

    expect(onNavigate).toHaveBeenCalledWith("history")
  })

  it("pins the nav, pagination, and footer around the list", () => {
    renderNav({ expanded: true, totalSessions: 120 })

    const area = scrollArea()
    expect(area).not.toBeNull()
    expect(area!.className).toContain("min-h-0")
    expect(area!.className).toContain("flex-1")

    const root = content()
    expect(area!.parentElement).toBe(root)

    const navGroup = screen
      .getByTestId("nav-compress")
      .closest('[data-slot="sidebar-group"]')
    const pagination = screen.getByText(/Page 1 of/)
    const footer = screen
      .getByTestId("nav-settings")
      .closest('[data-slot="sidebar-footer"]')

    for (const sibling of [navGroup, pagination, footer]) {
      expect(sibling).not.toBeNull()
      expect(area).not.toContainElement(sibling as HTMLElement)
    }

    expect(navGroup!.parentElement).toBe(root)
    expect(footer!.parentElement).toBe(root)
    expect(pagination.closest('[data-slot="sidebar-content"]')).toBe(root)
  })
})
