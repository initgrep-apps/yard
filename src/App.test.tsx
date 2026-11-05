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

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import type { ReactNode } from "react"
import { invoke } from "@tauri-apps/api/core"
import { open } from "@tauri-apps/plugin-dialog"
import { getCurrentWindow } from "@tauri-apps/api/window"

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }))
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: () => Promise.resolve(() => {}) }),
}))
vi.mock("@tauri-apps/api/window", async () => {
  const { mockWindow } = await import("@/lib/test-tauri")
  const win = mockWindow()
  return { getCurrentWindow: () => win }
})
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  confirm: vi.fn(),
}))
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
  Toaster: () => null,
}))
vi.mock("@/providers/HistoryProvider", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/providers/HistoryProvider")>()

  function SelectionProbe() {
    const { response, selectSession } = actual.useHistoryContext()
    const second = response?.sessions[1]
    if (!second) return null
    return (
      <button onClick={() => selectSession(second.sessionId)}>
        Select second session
      </button>
    )
  }

  return {
    ...actual,
    HistoryProvider: ({ children }: { children: ReactNode }) => (
      <actual.HistoryProvider>
        {children}
        <SelectionProbe />
      </actual.HistoryProvider>
    ),
  }
})

import App from "@/App"
import { PlatformProvider } from "@/providers/PlatformProvider"
import { batchLabel } from "@/lib/historyFormat"
import { mockWindow } from "@/lib/test-tauri"
import type { Platform } from "@/lib/platform"
import type {
  FileEntry,
  HistoryResponse,
  HistorySession,
  Settings,
} from "@/types"

const win = getCurrentWindow() as unknown as ReturnType<typeof mockWindow>

const settings: Settings = {
  outputFolder: "/Users/test/Documents/Compressed",
  categoryTargets: {
    image: { size: 200, unit: "KB" },
    video: { size: 8, unit: "MB" },
    pdf: { size: 1, unit: "MB" },
    document: { size: 2, unit: "MB" },
  },
  qualityFloors: {
    image: { minQuality: 30, minLongEdge: 1280 },
    video: { minCrf: 28, minResolution: "720p" },
    pdf: { minDpi: 72, minJpegQuality: 15 },
    document: { minMediaQuality: 40 },
  },
  performance: { adaptiveMemory: true, memoryCeilingMb: 2048 },
  history: { retentionDays: 90 },
}

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

function makeEntry(n: number): FileEntry {
  return {
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
  }
}

function makeSession(startedAt: number, sessionId = "s1"): HistorySession {
  return {
    sessionId,
    startedAt,
    endedAt: startedAt + 1_000,
    outputFolder: "/out",
    fileCount: 2,
    files: [],
    outcome: {
      done: 2,
      bestEffort: 0,
      failed: 0,
      cancelled: 0,
      unsupported: 0,
      alreadyUnderTarget: 0,
    },
    totalSavedBytes: 1_024,
    cancelled: false,
    interrupted: false,
    matchedFiles: null,
  }
}

function stubInvoke(history: HistoryResponse = emptyHistory) {
  vi.mocked(invoke).mockImplementation((command: unknown) => {
    switch (command) {
      case "get_history":
        return Promise.resolve(history)
      case "get_settings":
        return Promise.resolve(structuredClone(settings))
      case "add_files":
        return Promise.resolve([makeEntry(1), makeEntry(2), makeEntry(3)])
      default:
        return Promise.resolve(undefined)
    }
  })
}

const renderApp = (platform: Platform = "darwin") =>
  render(
    <PlatformProvider platform={platform}>
      <App />
    </PlatformProvider>,
  )

const topBar = () => screen.getByTestId("top-bar")
const sidebar = () =>
  document.querySelector('[data-slot="sidebar"]') as HTMLElement | null
const sidebarHeader = () =>
  document.querySelector('[data-slot="sidebar-header"]') as HTMLElement

beforeEach(() => {
  vi.resetAllMocks()
  stubInvoke()
  vi.mocked(open).mockResolvedValue(null)
  win.onCloseRequested.mockResolvedValue(vi.fn())
  win.startDragging.mockResolvedValue(undefined)
  win.toggleMaximize.mockResolvedValue(undefined)
  win.minimize.mockResolvedValue(undefined)
  win.close.mockResolvedValue(undefined)
  win.setTheme.mockResolvedValue(undefined)
})

afterEach(() => {
  window.innerWidth = 1024
})

describe("App shell", () => {
  it("renders a two-entry sidebar with no brand, version, or license chrome", () => {
    renderApp()

    const root = sidebar()!
    expect(root).not.toBeNull()
    expect(root.getAttribute("data-variant")).toBe("sidebar")
    expect(
      within(root).getByRole("button", { name: /Compress/ }),
    ).toBeInTheDocument()
    expect(
      within(root).getByRole("button", { name: /History/ }),
    ).toBeInTheDocument()

    expect(screen.queryByText("Yard")).not.toBeInTheDocument()
    expect(screen.queryByText("v1.0.0")).not.toBeInTheDocument()
    expect(screen.queryByText("GPLv3")).not.toBeInTheDocument()

    const settingsRows = screen.getAllByTestId("nav-settings")
    expect(settingsRows).toHaveLength(1)
    expect(
      settingsRows[0].closest('[data-slot="sidebar-footer"]'),
    ).not.toBeNull()

    expect(document.querySelector('[data-slot="sidebar-rail"]')).toBeNull()
    expect(
      document.querySelector('[data-slot="sidebar-content"]')!.className,
    ).toContain("overflow-hidden")
  })

  it("moves the toggle from the sidebar header to the top bar when collapsed", async () => {
    const user = userEvent.setup()
    renderApp("darwin")

    expect(
      within(sidebarHeader()).getByRole("button", {
        name: "Collapse sidebar",
      }),
    ).toBeInTheDocument()
    expect(
      within(sidebarHeader()).getByTestId("sidebar-actions"),
    ).toBeInTheDocument()
    expect(
      within(topBar()).queryByRole("button", { name: /sidebar/i }),
    ).toBeNull()
    expect(
      within(topBar()).queryByRole("button", { name: "Open navigation" }),
    ).toBeNull()
    expect(
      within(topBar()).queryByRole("button", { name: "Minimize" }),
    ).toBeNull()
    expect(topBar().className).not.toContain("pl-22")

    await user.click(
      within(sidebarHeader()).getByRole("button", {
        name: "Collapse sidebar",
      }),
    )

    const root = sidebar()!
    expect(root.getAttribute("data-state")).toBe("collapsed")
    expect(root.getAttribute("data-collapsible")).toBe("offcanvas")
    expect(topBar().className).toContain("pl-22")
    expect(
      within(topBar()).getByRole("button", { name: "Expand sidebar" }),
    ).toBeInTheDocument()
    expect(
      within(topBar()).getByRole("button", { name: "Open navigation" }),
    ).toBeInTheDocument()
    expect(within(topBar()).getByText("Compress")).toBeInTheDocument()
    expect(document.activeElement).toBe(
      within(topBar()).getByRole("button", { name: "Expand sidebar" }),
    )

    await user.click(
      within(topBar()).getByRole("button", { name: "Expand sidebar" }),
    )

    expect(document.activeElement).toBe(
      within(sidebarHeader()).getByRole("button", {
        name: "Collapse sidebar",
      }),
    )
  })

  it("shows window controls on Windows and hides the top-bar toggle while visible", async () => {
    const user = userEvent.setup()
    renderApp("win32")

    expect(sidebarHeader().firstElementChild!.childElementCount).toBe(0)

    const bar = topBar()
    expect(
      within(bar).getByRole("button", { name: "Minimize" }),
    ).toBeInTheDocument()
    expect(
      within(bar).getByRole("button", { name: "Maximize" }),
    ).toBeInTheDocument()
    expect(within(bar).getByRole("button", { name: "Close" })).toBeInTheDocument()
    expect(within(bar).queryByRole("button", { name: /sidebar/i })).toBeNull()
    expect(
      document.querySelector('[data-slot="sidebar-trigger"]'),
    ).not.toBeNull()

    await user.click(
      document.querySelector('[data-slot="sidebar-trigger"]') as HTMLElement,
    )

    expect(
      within(topBar()).getByRole("button", { name: "Expand sidebar" }),
    ).toBeInTheDocument()
    expect(
      within(topBar()).getByRole("button", { name: "Open navigation" }),
    ).toBeInTheDocument()
    expect(
      within(topBar()).getByRole("button", { name: "Close" }),
    ).toBeInTheDocument()
    expect(topBar().className).not.toContain("pl-22")
  })

  it("updates the breadcrumb when switching views", async () => {
    const user = userEvent.setup()
    const session = makeSession(new Date().getTime())
    stubInvoke({
      ...emptyHistory,
      sessions: [session],
      totalSessions: 1,
      totalFiles: 2,
    })
    renderApp("darwin")

    expect(within(topBar()).getByText("Compress")).toBeInTheDocument()

    await user.click(await screen.findByTestId("batch-s1"))

    expect(within(topBar()).getByText("History")).toBeInTheDocument()
    expect(within(topBar()).queryByText("Compress")).toBeNull()

    await user.click(
      within(sidebar()!).getByRole("button", { name: /Compress/ }),
    )

    expect(within(topBar()).getByText("Compress")).toBeInTheDocument()
  })

  it("adds the selected batch label to the History breadcrumb", async () => {
    const user = userEvent.setup()
    const started = new Date()
    started.setHours(10, 30, 0, 0)
    const session = makeSession(started.getTime())
    stubInvoke({
      ...emptyHistory,
      sessions: [session],
      totalSessions: 1,
      totalFiles: 2,
    })
    renderApp("darwin")

    await user.click(await screen.findByTestId("batch-s1"))

    const label = batchLabel(started.getTime(), new Date())
    await waitFor(() =>
      expect(within(topBar()).getByText(label)).toBeInTheDocument(),
    )
    expect(within(topBar()).getByText(label).className).toContain("truncate")
  })

  it("quit from the app menu closes the window instead of invoking quit", async () => {
    const user = userEvent.setup()
    renderApp("win32")

    await user.click(within(topBar()).getByRole("button", { name: "App menu" }))
    await user.click(await screen.findByRole("menuitem", { name: "Quit" }))

    expect(win.close).toHaveBeenCalledTimes(1)
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("quit")
    expect(win.onCloseRequested).toHaveBeenCalled()
  })

  it("keeps the mobile Sheet open while History expands and closes it from the header", async () => {
    window.innerWidth = 760
    const user = userEvent.setup()
    renderApp("darwin")

    await waitFor(() =>
      expect(document.querySelector('[data-slot="sidebar-gap"]')).toBeNull(),
    )
    expect(topBar().className).toContain("pl-22")

    await user.click(
      within(topBar()).getByRole("button", { name: "Expand sidebar" }),
    )

    const sheet = await waitFor(() => {
      const element = document.querySelector(
        '[data-slot="sidebar"][data-mobile="true"]',
      )
      expect(element).not.toBeNull()
      return element as HTMLElement
    })

    expect(
      within(sheet).getByRole("button", { name: "Collapse sidebar" }),
    ).toBeInTheDocument()

    await user.click(within(sheet).getByRole("button", { name: /History/ }))

    expect(
      document.querySelector('[data-slot="sidebar"][data-mobile="true"]'),
    ).not.toBeNull()
    expect(
      within(sheet).getByRole("button", { name: /History/ }),
    ).toHaveAttribute("aria-expanded", "true")
    expect(within(topBar()).getByText("Compress")).toBeInTheDocument()

    await user.click(
      within(sheet).getByRole("button", { name: "Collapse sidebar" }),
    )

    await waitFor(() =>
      expect(
        document.querySelector('[data-slot="sidebar"][data-mobile="true"]'),
      ).toBeNull(),
    )
    expect(topBar().className).toContain("pl-22")
  })

  it("keeps Compress and shows the empty batch list when History has no sessions", async () => {
    const user = userEvent.setup()
    renderApp("darwin")

    const row = within(sidebar()!).getByRole("button", { name: /History/ })

    await user.click(row)
    expect(within(sidebar()!).queryByText("No batches yet.")).toBeNull()

    await user.click(row)
    expect(screen.getByTestId("view-container")).toHaveAttribute(
      "data-view",
      "compress",
    )
    expect(within(sidebar()!).getByText("No batches yet.")).toBeInTheDocument()
  })

  it("caps the shell height and keeps the view container clipped", () => {
    renderApp("darwin")

    const wrapper = document.querySelector('[data-slot="sidebar-wrapper"]')
    expect(wrapper).not.toBeNull()
    expect(wrapper!.classList.contains("h-svh")).toBe(true)
    expect(wrapper!.classList.contains("overflow-hidden")).toBe(true)

    const viewContainer = screen.getByTestId("view-container")
    expect(viewContainer.className).toContain("min-h-0")
    expect(viewContainer.className).toContain("overflow-hidden")
  })

  it("shows the staged file count on the Compress nav row", async () => {
    const user = userEvent.setup()
    vi.mocked(open).mockResolvedValueOnce([
      "/t/f1.jpg",
      "/t/f2.jpg",
      "/t/f3.jpg",
    ])
    renderApp("darwin")

    await user.click(screen.getByRole("button", { name: /drop files/i }))

    expect(await screen.findByTestId("nav-count")).toHaveTextContent("3")
  })

  it("does not start dragging from the sidebar header trigger", () => {
    renderApp("win32")

    const trigger = document.querySelector(
      '[data-slot="sidebar-trigger"]',
    ) as HTMLElement
    fireEvent.mouseDown(trigger, { buttons: 1, detail: 1 })

    expect(win.startDragging).not.toHaveBeenCalled()
  })

  it("opens Settings from the sidebar footer as a dialog", async () => {
    const user = userEvent.setup()
    renderApp("darwin")

    await user.click(screen.getByTestId("nav-settings"))

    const dialog = await screen.findByRole("dialog")
    expect(within(dialog).getByText("Settings")).toBeInTheDocument()
    expect(
      await within(dialog).findByRole("tab", { name: "General" }),
    ).toBeInTheDocument()
  })

  it("closes the mobile Sheet with the header trigger", async () => {
    window.innerWidth = 760
    const user = userEvent.setup()
    renderApp("darwin")

    await waitFor(() =>
      expect(document.querySelector('[data-slot="sidebar-gap"]')).toBeNull(),
    )
    await user.click(
      within(topBar()).getByRole("button", { name: "Expand sidebar" }),
    )
    const sheet = await waitFor(() => {
      const element = document.querySelector(
        '[data-slot="sidebar"][data-mobile="true"]',
      )
      expect(element).not.toBeNull()
      return element as HTMLElement
    })
    const trigger = sheet.querySelector(
      '[data-slot="sidebar-trigger"]',
    ) as HTMLElement
    expect(trigger).not.toBeNull()

    await user.click(trigger)

    await waitFor(() =>
      expect(
        document.querySelector('[data-slot="sidebar"][data-mobile="true"]'),
      ).toBeNull(),
    )
  })

  it("closes the mobile Sheet when Settings is chosen", async () => {
    window.innerWidth = 760
    const user = userEvent.setup()
    renderApp("darwin")

    await waitFor(() =>
      expect(document.querySelector('[data-slot="sidebar-gap"]')).toBeNull(),
    )
    await user.click(
      within(topBar()).getByRole("button", { name: "Expand sidebar" }),
    )
    const sheet = await waitFor(() => {
      const element = document.querySelector(
        '[data-slot="sidebar"][data-mobile="true"]',
      )
      expect(element).not.toBeNull()
      return element as HTMLElement
    })

    await user.click(within(sheet).getByTestId("nav-settings"))

    await waitFor(() =>
      expect(
        document.querySelector('[data-slot="sidebar"][data-mobile="true"]'),
      ).toBeNull(),
    )
    expect(await screen.findByRole("dialog")).toBeInTheDocument()
  })

  it("opens the filter from the History row and closes it when the sidebar collapses", async () => {
    const user = userEvent.setup()
    renderApp("darwin")

    await user.click(
      within(sidebar()!).getByRole("button", { name: "Filter history" }),
    )

    const dialog = await screen.findByRole("dialog")
    expect(
      within(dialog).getByRole("heading", { name: "Filter history" }),
    ).toBeInTheDocument()

    fireEvent.keyDown(window, { key: "b", ctrlKey: true })

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  })

  it("applies the history filter and reseeds the dialog from it", async () => {
    const user = userEvent.setup()
    renderApp("darwin")

    await user.click(
      within(sidebar()!).getByRole("button", { name: "Filter history" }),
    )
    const dialog = await screen.findByRole("dialog")
    await user.click(within(dialog).getByRole("button", { name: "Failed" }))
    await user.click(within(dialog).getByRole("button", { name: "Apply" }))

    await waitFor(() =>
      expect(screen.queryByRole("dialog")).toBeNull(),
    )

    await waitFor(() => {
      const calls = vi
        .mocked(invoke)
        .mock.calls.filter(([command]) => command === "get_history")
      const last = calls[calls.length - 1]?.[1] as
        | { filters?: { statuses?: string[] } }
        | undefined
      expect(last?.filters?.statuses).toEqual(["failed"])
    })

    await user.click(
      within(sidebar()!).getByRole("button", { name: "Filter history" }),
    )
    const reopened = await screen.findByRole("dialog")
    expect(
      within(reopened).getByRole("button", { name: "Failed" }),
    ).toHaveAttribute("aria-pressed", "true")
  })

  it("closes the mobile Sheet when a batch row is selected", async () => {
    window.innerWidth = 760
    const user = userEvent.setup()
    const session = makeSession(new Date().setHours(10, 30, 0, 0))
    stubInvoke({
      ...emptyHistory,
      sessions: [session],
      totalSessions: 1,
      totalFiles: 2,
    })
    renderApp("darwin")

    await waitFor(() =>
      expect(document.querySelector('[data-slot="sidebar-gap"]')).toBeNull(),
    )
    await user.click(
      within(topBar()).getByRole("button", { name: "Expand sidebar" }),
    )
    const sheet = await waitFor(() => {
      const element = document.querySelector(
        '[data-slot="sidebar"][data-mobile="true"]',
      )
      expect(element).not.toBeNull()
      return element as HTMLElement
    })

    await user.click(within(sheet).getByTestId(`batch-${session.sessionId}`))

    await waitFor(() =>
      expect(
        document.querySelector('[data-slot="sidebar"][data-mobile="true"]'),
      ).toBeNull(),
    )
  })

  it("renders the matching view when navigating", async () => {
    const user = userEvent.setup()
    const session = makeSession(new Date().getTime())
    stubInvoke({
      ...emptyHistory,
      sessions: [session],
      totalSessions: 1,
      totalFiles: 2,
    })
    renderApp("darwin")

    expect(screen.getByTestId("view-container")).toHaveAttribute(
      "data-view",
      "compress",
    )
    expect(
      screen.getByRole("button", { name: /drop files/i }),
    ).toBeInTheDocument()

    await user.click(await screen.findByTestId("batch-s1"))

    expect(screen.getByTestId("view-container")).toHaveAttribute(
      "data-view",
      "history",
    )
    expect(screen.queryByRole("button", { name: /drop files/i })).toBeNull()

    await user.click(
      within(sidebar()!).getByRole("button", { name: /Compress/ }),
    )

    expect(screen.getByTestId("view-container")).toHaveAttribute(
      "data-view",
      "compress",
    )
    expect(
      screen.getByRole("button", { name: /drop files/i }),
    ).toBeInTheDocument()
  })

  it("shows the selected batch label only on History", async () => {
    const user = userEvent.setup()
    const first = new Date()
    first.setHours(10, 30, 0, 0)
    const second = new Date()
    second.setHours(11, 45, 0, 0)
    const firstSession = makeSession(first.getTime(), "s1")
    const secondSession = makeSession(second.getTime(), "s2")
    stubInvoke({
      ...emptyHistory,
      sessions: [firstSession, secondSession],
      totalSessions: 2,
      totalFiles: 4,
    })
    renderApp("darwin")

    await user.click(await screen.findByTestId("batch-s2"))

    const secondLabel = batchLabel(secondSession.startedAt, new Date())
    const firstLabel = batchLabel(firstSession.startedAt, new Date())
    await waitFor(() =>
      expect(within(topBar()).getByText(secondLabel)).toBeInTheDocument(),
    )
    expect(within(topBar()).queryByText(firstLabel)).toBeNull()

    await user.click(
      within(sidebar()!).getByRole("button", { name: /Compress/ }),
    )

    expect(within(topBar()).queryByText(secondLabel)).toBeNull()
    expect(within(topBar()).getByText("Compress")).toBeInTheDocument()
  })

  it("treats an unknown platform as non-darwin chrome", () => {
    renderApp("unknown")

    const bar = topBar()
    expect(
      within(bar).getByRole("button", { name: "Minimize" }),
    ).toBeInTheDocument()
    expect(bar.className).not.toContain("pl-22")
    expect(
      document.querySelector('[data-slot="sidebar-trigger"]'),
    ).not.toBeNull()
  })

  it("opens About from the app menu", async () => {
    const user = userEvent.setup()
    renderApp("darwin")

    await user.click(within(topBar()).getByRole("button", { name: "App menu" }))
    await user.click(await screen.findByRole("menuitem", { name: "About" }))

    const dialog = await screen.findByRole("dialog")
    expect(
      within(dialog).getByRole("heading", { name: "Yard" }),
    ).toBeInTheDocument()
  })

  it("closes and reopens Settings with Escape", async () => {
    const user = userEvent.setup()
    renderApp("darwin")

    await user.click(screen.getByTestId("nav-settings"))
    expect(await screen.findByRole("dialog")).toBeInTheDocument()

    await user.keyboard("{Escape}")
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())

    await user.click(screen.getByTestId("nav-settings"))
    expect(await screen.findByRole("dialog")).toBeInTheDocument()
  })

  it("closes and reopens About with Escape", async () => {
    const user = userEvent.setup()
    renderApp("darwin")

    await user.click(within(topBar()).getByRole("button", { name: "App menu" }))
    await user.click(await screen.findByRole("menuitem", { name: "About" }))
    expect(await screen.findByRole("dialog")).toBeInTheDocument()

    await user.keyboard("{Escape}")
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())

    await user.click(within(topBar()).getByRole("button", { name: "App menu" }))
    await user.click(await screen.findByRole("menuitem", { name: "About" }))
    expect(await screen.findByRole("dialog")).toBeInTheDocument()
  })

  it("wires the collapsed ☰ menu to Settings and History", async () => {
    const user = userEvent.setup()
    renderApp("darwin")

    await user.click(
      within(sidebarHeader()).getByRole("button", {
        name: "Collapse sidebar",
      }),
    )
    await user.click(
      within(topBar()).getByRole("button", { name: "Open navigation" }),
    )
    await user.click(await screen.findByRole("menuitem", { name: "Settings" }))

    const dialog = await screen.findByRole("dialog")
    expect(within(dialog).getByText("Settings")).toBeInTheDocument()

    await user.keyboard("{Escape}")
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())

    await user.click(
      within(topBar()).getByRole("button", { name: "Open navigation" }),
    )
    await user.click(await screen.findByRole("menuitem", { name: "History" }))

    expect(screen.getByTestId("view-container")).toHaveAttribute(
      "data-view",
      "history",
    )
    expect(within(topBar()).getByText("History")).toBeInTheDocument()
  })

  it("does not drag or maximize the window from interactive header controls", async () => {
    const user = userEvent.setup()
    renderApp("win32")

    const headerTrigger = document.querySelector(
      '[data-slot="sidebar-trigger"]',
    ) as HTMLElement
    fireEvent.doubleClick(headerTrigger)
    expect(win.toggleMaximize).not.toHaveBeenCalled()

    await user.click(headerTrigger)

    const expandButton = within(topBar()).getByRole("button", {
      name: "Expand sidebar",
    })
    const navigationButton = within(topBar()).getByRole("button", {
      name: "Open navigation",
    })
    const themeToggle = within(topBar()).getByRole("button", {
      name: "Toggle theme",
    })
    const appMenu = within(topBar()).getByRole("button", {
      name: "App menu",
    })

    for (const control of [
      expandButton,
      navigationButton,
      themeToggle,
      appMenu,
    ]) {
      fireEvent.doubleClick(control)
      fireEvent.mouseDown(control, { buttons: 1, detail: 1 })
    }

    expect(win.toggleMaximize).not.toHaveBeenCalled()
    expect(win.startDragging).not.toHaveBeenCalled()

    fireEvent.doubleClick(topBar())
    expect(win.toggleMaximize).toHaveBeenCalledTimes(1)
  })
})
