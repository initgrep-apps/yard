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
import { getCurrentWindow } from "@tauri-apps/api/window"

const { historyMock, toastErrorMock } = vi.hoisted(() => ({
  historyMock: vi.fn(),
  toastErrorMock: vi.fn(),
}))

vi.mock("@tauri-apps/api/window", async () => {
  const { mockWindow } = await import("@/lib/test-tauri")
  const win = mockWindow()
  return { getCurrentWindow: () => win }
})
vi.mock("@/providers/HistoryProvider", () => ({
  useHistoryContext: historyMock,
}))
vi.mock("sonner", () => ({ toast: { error: toastErrorMock } }))

import { TopBar } from "@/components/shell/TopBar"
import { SidebarProvider } from "@/components/ui/sidebar"
import { PlatformProvider } from "@/providers/PlatformProvider"
import { batchLabel } from "@/lib/historyFormat"
import { mockWindow } from "@/lib/test-tauri"
import type { Platform } from "@/lib/platform"
import type { HistoryResponse, HistorySession } from "@/types"
import type { ViewId } from "@/types"

const win = getCurrentWindow() as unknown as ReturnType<typeof mockWindow>

function makeSession(startedAt: number): HistorySession {
  return {
    sessionId: "s1",
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
      needsPermission: 0,
    },
    totalSavedBytes: 1_024,
    cancelled: false,
    interrupted: false,
    matchedFiles: null,
  }
}

const emptyResponse: HistoryResponse = {
  sessions: [],
  page: 0,
  pageSize: 50,
  totalSessions: 0,
  totalFiles: 0,
  summary: { sessions: 0, files: 0, savedBytes: 0, failedFiles: 0 },
  outputFolders: [],
  skippedLines: 0,
}

function renderTopBar({
  platform = "darwin",
  open = true,
  activeView = "compress",
  session = null,
}: {
  platform?: Platform
  open?: boolean
  activeView?: ViewId
  session?: HistorySession | null
} = {}) {
  historyMock.mockReturnValue({
    response: session
      ? { ...emptyResponse, sessions: [session], totalSessions: 1 }
      : emptyResponse,
    selectedSessionId: session?.sessionId ?? null,
  })

  return render(
    <PlatformProvider platform={platform}>
      <SidebarProvider defaultOpen={open}>
        <TopBar
          activeView={activeView}
          onNavigate={vi.fn()}
          onOpenSettings={vi.fn()}
          onOpenAbout={vi.fn()}
        />
      </SidebarProvider>
    </PlatformProvider>,
  )
}

const topBar = () => screen.getByTestId("top-bar")

beforeEach(() => {
  historyMock.mockReset()
  toastErrorMock.mockReset()
  win.startDragging.mockReset()
  win.toggleMaximize.mockReset()
  win.setTheme.mockReset()
  win.close.mockReset()
  win.startDragging.mockResolvedValue(undefined)
  win.toggleMaximize.mockResolvedValue(undefined)
  win.setTheme.mockResolvedValue(undefined)
  win.close.mockResolvedValue(undefined)
})

afterEach(() => {
  window.innerWidth = 1024
})

describe("TopBar", () => {
  it("leaves the toggle to the sidebar and keeps the breadcrumb on macOS", () => {
    renderTopBar()

    const bar = topBar()
    expect(
      within(bar).queryByRole("button", { name: /sidebar/i }),
    ).toBeNull()
    expect(
      within(bar).queryByRole("button", { name: "Open navigation" }),
    ).toBeNull()
    expect(within(bar).queryByRole("button", { name: "Minimize" })).toBeNull()
    expect(
      within(bar).getByRole("button", { name: "Toggle theme" }),
    ).toBeInTheDocument()
    expect(
      within(bar).getByRole("button", { name: "App menu" }),
    ).toBeInTheDocument()
    expect(within(bar).getByText("Compress")).toBeInTheDocument()
    expect(bar.className).not.toContain("pl-22")
  })

  it("shows the toggle and ☰ menu with traffic-light clearance on macOS when hidden", () => {
    renderTopBar({ open: false })

    const bar = topBar()
    expect(
      within(bar).getByRole("button", { name: "Expand sidebar" }),
    ).toBeInTheDocument()
    expect(
      within(bar).getByRole("button", { name: "Open navigation" }),
    ).toBeInTheDocument()
    expect(bar.className).toContain("pl-22")
  })

  it("hides the top-bar toggle on Windows while visible and keeps window controls", () => {
    renderTopBar({ platform: "win32" })

    const bar = topBar()
    expect(
      within(bar).queryByRole("button", { name: /sidebar/i }),
    ).toBeNull()
    expect(
      within(bar).getByRole("button", { name: "Minimize" }),
    ).toBeInTheDocument()
    expect(
      within(bar).getByRole("button", { name: "Maximize" }),
    ).toBeInTheDocument()
    expect(within(bar).getByRole("button", { name: "Close" })).toBeInTheDocument()
    expect(bar.className).not.toContain("pl-22")
  })

  it("shows the toggle and ☰ on Windows when hidden, without traffic-light clearance", () => {
    renderTopBar({ platform: "win32", open: false })

    const bar = topBar()
    expect(
      within(bar).getByRole("button", { name: "Expand sidebar" }),
    ).toBeInTheDocument()
    expect(
      within(bar).getByRole("button", { name: "Open navigation" }),
    ).toBeInTheDocument()
    expect(within(bar).getByRole("button", { name: "Close" })).toBeInTheDocument()
    expect(bar.className).not.toContain("pl-22")
  })

  it("renders History and the selected batch label, truncated", () => {
    const started = new Date()
    started.setHours(10, 30, 0, 0)
    const session = makeSession(started.getTime())
    const label = batchLabel(started.getTime(), new Date())

    renderTopBar({ activeView: "history", session })

    const bar = topBar()
    expect(within(bar).getByText("History")).toBeInTheDocument()
    expect(within(bar).queryByText("Compress")).toBeNull()
    const labelEl = within(bar).getByText(label)
    expect(labelEl.className).toContain("truncate")
  })

  it("starts dragging from the bar but not from its buttons", () => {
    renderTopBar({ open: false })

    fireEvent.mouseDown(
      within(topBar()).getByRole("button", { name: "Expand sidebar" }),
      { buttons: 1, detail: 1 },
    )
    expect(win.startDragging).not.toHaveBeenCalled()

    fireEvent.mouseDown(topBar(), { buttons: 1, detail: 1 })
    expect(win.startDragging).toHaveBeenCalledTimes(1)
  })

  it("reports a failed quit and keeps the window open", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {})
    try {
      win.close.mockRejectedValueOnce(new Error("denied"))
      const user = userEvent.setup()
      renderTopBar({ platform: "win32" })

      await user.click(
        within(topBar()).getByRole("button", { name: "App menu" }),
      )
      await user.click(await screen.findByRole("menuitem", { name: "Quit" }))

      await waitFor(() =>
        expect(toastErrorMock).toHaveBeenCalledWith(
          "Could not quit the app — the window stays open.",
          { id: "window-close-failed" },
        ),
      )
      expect(win.close).toHaveBeenCalledTimes(1)
      expect(consoleError).toHaveBeenCalledWith(
        "window close failed:",
        expect.any(Error),
      )
    } finally {
      consoleError.mockRestore()
    }
  })

  it("uses the mobile overlay state for visibility decisions", async () => {
    window.innerWidth = 760
    renderTopBar({ platform: "darwin", open: true })

    await waitFor(() => {
      expect(
        within(topBar()).getByRole("button", { name: "Expand sidebar" }),
      ).toBeInTheDocument()
    })
    expect(topBar().className).toContain("pl-22")

    await userEvent
      .setup()
      .click(within(topBar()).getByRole("button", { name: "Expand sidebar" }))

    expect(
      within(topBar()).queryByRole("button", { name: /sidebar/i }),
    ).toBeNull()
    expect(
      within(topBar()).queryByRole("button", { name: "Open navigation" }),
    ).toBeNull()
    expect(topBar().className).not.toContain("pl-22")
  })
})
