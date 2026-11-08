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

import { afterEach, describe, expect, it, vi } from "vitest"
import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { version } from "../../../package.json"

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(),
}))
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}))

import { openUrl } from "@tauri-apps/plugin-opener"
import type { Update } from "@tauri-apps/plugin-updater"
import { formatSize } from "@/lib/sizeFormat"
import {
  UpdateContext,
  type UpdateContextValue,
  type UpdateState,
} from "@/providers/UpdateProvider"
import { UpdateSection } from "@/components/update/UpdateSection"

const openUrlMock = vi.mocked(openUrl)

afterEach(() => {
  vi.clearAllMocks()
})

function fakeUpdate(version = "1.2.0"): Update {
  return {
    version,
    currentVersion: "1.1.0",
    date: "2026-11-08T00:00:00Z",
    body: "",
  } as unknown as Update
}

function fakeContext(overrides: Partial<UpdateContextValue> = {}): UpdateContextValue {
  return {
    state: { status: "idle" },
    canCheck: true,
    isCompressing: false,
    check: vi.fn(async () => {}),
    startDownload: vi.fn(async () => {}),
    install: vi.fn(async () => {}),
    decline: vi.fn(async () => {}),
    retry: vi.fn(async () => {}),
    setSurfaceOpen: vi.fn(),
    ...overrides,
  }
}

function renderSection(overrides: Partial<UpdateContextValue> = {}) {
  const value = fakeContext(overrides)
  return {
    value,
    ...render(
      <UpdateContext.Provider value={value}>
        <UpdateSection />
      </UpdateContext.Provider>,
    ),
  }
}

describe("UpdateSection", () => {
  it("shows the version and check button", () => {
    renderSection({ state: { status: "idle" } })
    expect(screen.getByText(`Version ${version}`)).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeEnabled()
  })

  it("shows the up-to-date state", () => {
    renderSection({ state: { status: "current" } })
    expect(screen.getByText("You're on the latest version.")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Check again" })).toBeInTheDocument()
  })

  it("shows update details and actions", () => {
    renderSection({
      state: { status: "available", update: fakeUpdate("1.2.0") },
    })
    expect(screen.getByText("Version 1.2.0 is available")).toBeInTheDocument()
    expect(screen.getByText("Released Nov 8, 2026.")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Update now" })).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Not now" })).toBeInTheDocument()
  })

  it("calls check, download, decline, and install", async () => {
    const user = userEvent.setup()

    const idle = renderSection({ state: { status: "idle" } })
    await user.click(screen.getByRole("button", { name: "Check for updates" }))
    expect(idle.value.check).toHaveBeenCalledTimes(1)
    idle.unmount()

    const available = renderSection({
      state: { status: "available", update: fakeUpdate() },
    })
    await user.click(screen.getByRole("button", { name: "Update now" }))
    expect(available.value.startDownload).toHaveBeenCalledTimes(1)
    await user.click(screen.getByRole("button", { name: "Not now" }))
    expect(available.value.decline).toHaveBeenCalledTimes(1)
    available.unmount()

    const ready = renderSection({
      state: { status: "ready", update: fakeUpdate() },
    })
    await user.click(screen.getByRole("button", { name: "Restart app" }))
    expect(ready.value.install).toHaveBeenCalledTimes(1)
  })

  it("opens the release page from What's new", async () => {
    const user = userEvent.setup()
    renderSection({
      state: { status: "available", update: fakeUpdate("1.2.0") },
    })
    await user.click(screen.getByRole("button", { name: "What's new" }))
    await waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith(
        "https://github.com/initgrep-apps/yard/releases/tag/v1.2.0",
      )
    })
  })

  it("shows download progress and the background note", () => {
    renderSection({
      state: {
        status: "downloading",
        update: fakeUpdate(),
        downloaded: 42_000_000,
        total: 100_000_000,
      },
    })
    expect(screen.getByText("Downloading update…")).toBeInTheDocument()
    expect(
      screen.getByText(
        `42% · ${formatSize(42_000_000)} of ${formatSize(100_000_000)}`,
      ),
    ).toBeInTheDocument()
    expect(
      screen.getByText(
        "You can close this window. The download continues in the background.",
      ),
    ).toBeInTheDocument()
    expect(screen.getByRole("progressbar")).toBeInTheDocument()
  })

  it("shows download bytes without a percentage when the total is unknown", () => {
    const { container } = renderSection({
      state: {
        status: "downloading",
        update: fakeUpdate(),
        downloaded: 13_002_342,
        total: null,
      },
    })
    expect(screen.getByText("Downloading update…")).toBeInTheDocument()
    expect(screen.getByText("12.4 MB")).toBeInTheDocument()
    expect(container.textContent).not.toMatch(/%/)
    expect(screen.getByRole("progressbar")).toBeInTheDocument()
  })

  it("shows the ready state", () => {
    renderSection({
      state: { status: "ready", update: fakeUpdate("1.2.0") },
    })
    expect(screen.getByText("Update ready.")).toBeInTheDocument()
    expect(
      screen.getByText("Restart the app to finish installing version 1.2.0."),
    ).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Restart app" })).toBeEnabled()
  })

  it("defers the restart while a batch is running", () => {
    renderSection({
      state: { status: "ready", update: fakeUpdate() },
      isCompressing: true,
    })
    expect(
      screen.getByText("Finish the current batch to restart."),
    ).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Restart app" })).toBeDisabled()
  })

  it("shows the installing state", () => {
    renderSection({
      state: { status: "installing", update: fakeUpdate() },
    })
    expect(screen.getByText("Installing update…")).toBeInTheDocument()
    expect(screen.getByText("The app will restart.")).toBeInTheDocument()
  })

  it("shows the error message in an alert with retry", () => {
    renderSection({
      state: {
        status: "error",
        stage: "download",
        message:
          "The update couldn't be downloaded. Check your connection and try again.",
        update: fakeUpdate(),
      },
    })
    expect(screen.getByRole("alert")).toHaveTextContent(
      "The update couldn't be downloaded. Check your connection and try again.",
    )
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument()
    expect(
      screen.getByRole("button", { name: "Download from GitHub" }),
    ).toBeInTheDocument()
  })

  it("disables checking in development builds", () => {
    renderSection({ state: { status: "idle" }, canCheck: false })
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeDisabled()
    expect(
      screen.getByText("Updates are disabled in development builds."),
    ).toBeInTheDocument()
  })

  it("never names the app in any state", () => {
    const states: UpdateState[] = [
      { status: "idle" },
      { status: "checking" },
      { status: "current" },
      { status: "available", update: fakeUpdate() },
      {
        status: "downloading",
        update: fakeUpdate(),
        downloaded: 1,
        total: 2,
      },
      { status: "ready", update: fakeUpdate() },
      { status: "installing", update: fakeUpdate() },
      {
        status: "error",
        stage: "check",
        message: "Couldn't check for updates. Check your connection and try again.",
        update: null,
      },
    ]

    for (const state of states) {
      const { container, unmount } = renderSection({ state })
      expect(container.textContent).not.toMatch(/Yard/)
      unmount()
    }
  })
})
