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

import { describe, it, expect, vi, afterEach } from "vitest"
import { render, screen, act, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(),
}))
vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(),
}))
vi.mock("sonner", () => ({
  toast: { custom: vi.fn(), dismiss: vi.fn(), error: vi.fn() },
}))

import { check } from "@tauri-apps/plugin-updater"
import { relaunch } from "@tauri-apps/plugin-process"
import type { Update, DownloadEvent } from "@tauri-apps/plugin-updater"
import { toast } from "sonner"

const checkMock = vi.mocked(check)
const relaunchMock = vi.mocked(relaunch)

import {
  CompressionContext,
  type CompressionContextValue,
} from "@/providers/CompressionProvider"
import { PlatformProvider } from "@/providers/PlatformProvider"
import { UpdateProvider, useUpdate } from "@/providers/UpdateProvider"
import type { Platform } from "@/lib/platform"

function fakeUpdate(version = "1.2.0"): Update {
  return {
    version,
    currentVersion: "1.1.0",
    date: "2026-11-08T00:00:00Z",
    body: "",
    download: vi.fn(async (onEvent: (event: DownloadEvent) => void) => {
      onEvent({ event: "Started", data: { contentLength: 100 } })
      onEvent({ event: "Progress", data: { chunkLength: 40 } })
      onEvent({ event: "Finished" })
    }),
    install: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
  } as unknown as Update
}

function stalledUpdate(events: Array<"Started" | "Progress">): Update {
  const update = fakeUpdate()
  update.download = vi.fn(async (onEvent: (event: DownloadEvent) => void) => {
    for (const event of events) {
      if (event === "Started") {
        onEvent({ event, data: { contentLength: 100 } })
      } else {
        onEvent({ event, data: { chunkLength: 40 } })
      }
    }
    await new Promise<void>(() => {})
  })
  return update
}

function Probe() {
  const { state, check, startDownload, install, decline, retry, setSurfaceOpen } = useUpdate()
  const version = "update" in state && state.update ? state.update.version : null
  return (
    <div>
      <span data-testid="status">
        {state.status}
        {version ? ` ${version}` : ""}
      </span>
      {state.status === "downloading" ? (
        <span data-testid="progress">{state.downloaded}</span>
      ) : null}
      {state.status === "error" ? (
        <span data-testid="message">{state.message}</span>
      ) : null}
      <button type="button" onClick={check}>
        check
      </button>
      <button type="button" onClick={startDownload}>
        download
      </button>
      <button type="button" onClick={install}>
        install
      </button>
      <button type="button" onClick={decline}>
        decline
      </button>
      <button type="button" onClick={retry}>
        retry
      </button>
      <button type="button" onClick={() => setSurfaceOpen(true)}>
        open surface
      </button>
      <button type="button" onClick={() => setSurfaceOpen(false)}>
        close surface
      </button>
    </div>
  )
}

function Harness({
  isCompressing = false,
  platform = "darwin",
}: {
  isCompressing?: boolean
  platform?: Platform
}) {
  return (
    <CompressionContext.Provider
      value={{ isCompressing } as unknown as CompressionContextValue}
    >
      <PlatformProvider platform={platform}>
        <UpdateProvider>
          <Probe />
        </UpdateProvider>
      </PlatformProvider>
    </CompressionContext.Provider>
  )
}

describe("UpdateProvider", () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it("starts idle and does not check on mount", () => {
    render(<Harness />)
    expect(screen.getByTestId("status")).toHaveTextContent("idle")
    expect(checkMock).not.toHaveBeenCalled()
  })

  it("moves idle -> checking -> available", async () => {
    const user = userEvent.setup()
    checkMock.mockResolvedValue(fakeUpdate("1.2.0"))
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    expect(await screen.findByTestId("status")).toHaveTextContent("available 1.2.0")
  })

  it("moves checking -> current when no update exists", async () => {
    const user = userEvent.setup()
    checkMock.mockResolvedValue(null)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    expect(await screen.findByTestId("status")).toHaveTextContent("current")
  })

  it("moves checking -> error when the check fails", async () => {
    const user = userEvent.setup()
    checkMock.mockRejectedValue(new Error("offline"))
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    expect(await screen.findByTestId("status")).toHaveTextContent("error")
    expect(screen.getByTestId("message")).toHaveTextContent(
      "Couldn't check for updates. Check your connection and try again.",
    )
  })

  it("downloads, reports progress, then installs and relaunches on macOS", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    let finishDownload: () => void = () => {}
    update.download = vi.fn(async (onEvent: (event: DownloadEvent) => void) => {
      onEvent({ event: "Started", data: { contentLength: 100 } })
      onEvent({ event: "Progress", data: { chunkLength: 40 } })
      await new Promise<void>((resolve) => {
        finishDownload = resolve
      })
      onEvent({ event: "Finished" })
    })
    checkMock.mockResolvedValue(update)
    render(<Harness platform="darwin" />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("progress")).toHaveTextContent("40")
    await act(async () => finishDownload())
    await waitFor(() => expect(update.install).toHaveBeenCalled())
    expect(relaunchMock).toHaveBeenCalled()
  })

  it("defers install when a batch is compressing", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    checkMock.mockResolvedValue(update)
    const view = render(<Harness isCompressing />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("status")).toHaveTextContent("ready")
    expect(update.install).not.toHaveBeenCalled()
    view.rerender(<Harness isCompressing={false} />)
    await user.click(screen.getByRole("button", { name: "install" }))
    expect(update.install).toHaveBeenCalled()
  })

  it("does not relaunch on Windows", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    checkMock.mockResolvedValue(update)
    render(<Harness platform="win32" />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    await waitFor(() => expect(update.install).toHaveBeenCalled())
    expect(relaunchMock).not.toHaveBeenCalled()
  })

  it("closes the update on decline", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    checkMock.mockResolvedValue(update)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "decline" }))
    expect(update.close).toHaveBeenCalled()
    expect(screen.getByTestId("status")).toHaveTextContent("idle")
  })

  it("tells the user to move the app when install is denied", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    update.install = vi.fn(async () => {
      throw new Error("Permission denied")
    })
    checkMock.mockResolvedValue(update)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("message")).toHaveTextContent(
      "The app couldn't be replaced. Move it to Applications, then try again.",
    )
  })

  it("moves downloading -> error when the download fails", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    update.download = vi.fn(async () => {
      throw new Error("network")
    })
    checkMock.mockResolvedValue(update)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("message")).toHaveTextContent(
      "The update couldn't be downloaded. Check your connection and try again.",
    )
  })

  it("shows the generic install error", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    update.install = vi.fn(async () => {
      throw new Error("boom")
    })
    checkMock.mockResolvedValue(update)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("message")).toHaveTextContent(
      "The update couldn't be installed. Try again, or download the latest installer from GitHub.",
    )
  })

  it("retries the check", async () => {
    const user = userEvent.setup()
    checkMock
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(fakeUpdate("1.2.0"))
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    expect(await screen.findByTestId("status")).toHaveTextContent("error")
    await user.click(screen.getByRole("button", { name: "retry" }))
    expect(await screen.findByTestId("status")).toHaveTextContent("available 1.2.0")
  })

  it("retries the download", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    update.download = vi
      .fn()
      .mockRejectedValueOnce(new Error("network"))
      .mockResolvedValueOnce(undefined)
    checkMock.mockResolvedValue(update)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("message")).toHaveTextContent("downloaded")
    await user.click(screen.getByRole("button", { name: "retry" }))
    await waitFor(() => expect(update.install).toHaveBeenCalled())
  })

  it("retries the install", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    update.install = vi
      .fn()
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValueOnce(undefined)
    checkMock.mockResolvedValue(update)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("message")).toHaveTextContent("installed")
    await user.click(screen.getByRole("button", { name: "retry" }))
    await waitFor(() => expect(update.install).toHaveBeenCalledTimes(2))
    expect(relaunchMock).toHaveBeenCalled()
  })

  it("shows a background progress toast while downloading", async () => {
    const user = userEvent.setup()
    const update = stalledUpdate(["Started", "Progress"])
    checkMock.mockResolvedValue(update)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("progress")).toHaveTextContent("40")
    await waitFor(() =>
      expect(toast.custom).toHaveBeenCalledWith(expect.any(Function), {
        id: "updater-progress",
        position: "top-right",
        duration: Infinity,
      }),
    )
  })

  it("dismisses the progress toast when the update surface opens", async () => {
    const user = userEvent.setup()
    const update = stalledUpdate(["Started", "Progress"])
    checkMock.mockResolvedValue(update)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("progress")).toHaveTextContent("40")
    await user.click(screen.getByRole("button", { name: "open surface" }))
    await waitFor(() => expect(toast.dismiss).toHaveBeenCalledWith("updater-progress"))
  })

  it("does not show a toast while the update surface is open", async () => {
    const user = userEvent.setup()
    const update = stalledUpdate(["Started", "Progress"])
    checkMock.mockResolvedValue(update)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "open surface" }))
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("progress")).toHaveTextContent("40")
    expect(toast.custom).not.toHaveBeenCalled()
  })

  it("shows an error toast with a retry action", async () => {
    const user = userEvent.setup()
    checkMock.mockRejectedValue(new Error("offline"))
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Update failed", {
        id: "updater-error",
        description: "Couldn't check for updates. Check your connection and try again.",
        action: expect.objectContaining({ label: "Retry" }),
      }),
    )
  })

  it("throttles rapid download toast updates", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    update.download = vi.fn(async (onEvent: (event: DownloadEvent) => void) => {
      onEvent({ event: "Started", data: { contentLength: 100 } })
      for (let index = 0; index < 10; index += 1) {
        onEvent({ event: "Progress", data: { chunkLength: 10 } })
      }
      await new Promise<void>(() => {})
    })
    checkMock.mockResolvedValue(update)
    render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("progress")).toHaveTextContent("100")
    expect(vi.mocked(toast.custom).mock.calls.length).toBeLessThan(11)
  })

  it("keeps the update ready when retrying an install during a batch", async () => {
    const user = userEvent.setup()
    const update = fakeUpdate()
    update.install = vi
      .fn()
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValueOnce(undefined)
    checkMock.mockResolvedValue(update)
    const view = render(<Harness />)
    await user.click(screen.getByRole("button", { name: "check" }))
    await user.click(await screen.findByRole("button", { name: "download" }))
    expect(await screen.findByTestId("message")).toHaveTextContent("installed")
    view.rerender(<Harness isCompressing />)
    await user.click(screen.getByRole("button", { name: "retry" }))
    expect(screen.getByTestId("status")).toHaveTextContent("ready")
    expect(update.install).toHaveBeenCalledTimes(1)
  })
})
