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

import { useState } from "react"
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { invoke } from "@tauri-apps/api/core"
import { open } from "@tauri-apps/plugin-dialog"
import { toast } from "sonner"
import type { Settings } from "@/types"

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }))
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }))
vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), dismiss: vi.fn() },
}))
const { useHistoryContextMock } = vi.hoisted(() => ({
  useHistoryContextMock: vi.fn(),
}))
vi.mock("@/providers/HistoryProvider", () => ({
  useHistoryContext: useHistoryContextMock,
}))

import { SettingsDialog } from "@/components/SettingsDialog"

const baseSettings: Settings = {
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
  consent: { allowDestructive: false, heicToJpg: true },
}

const resetDefaults: Settings = {
  ...baseSettings,
  outputFolder: "/defaults/Compressed",
  history: { retentionDays: 30 },
  consent: { allowDestructive: false, heicToJpg: true },
}

const saveCalls = () =>
  vi.mocked(invoke).mock.calls.filter(([command]) => command === "save_settings")

const savedSettings = (index = 0) =>
  (saveCalls()[index]?.[1] as { settings: Settings }).settings

function deferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

function ForceCloseDialog() {
  const [open, setOpen] = useState(true)

  return (
    <div>
      <button onClick={() => setOpen(false)}>Force close</button>
      <button onClick={() => setOpen(true)}>Force open</button>
      <SettingsDialog open={open} onOpenChange={setOpen} />
    </div>
  )
}

async function openHistoryTab() {
  render(<SettingsDialog open onOpenChange={vi.fn()} />)
  await screen.findByRole("tab", { name: "General" })
  fireEvent.click(screen.getByRole("tab", { name: "History" }))
  return screen.getByLabelText("History retention (days)")
}

beforeEach(() => {
  vi.mocked(invoke).mockReset()
  vi.mocked(open).mockReset()
  vi.mocked(toast.error).mockReset()
  vi.mocked(toast.dismiss).mockReset()
  useHistoryContextMock.mockReturnValue({
    clearHistory: vi.fn().mockResolvedValue(undefined),
  })
  vi.mocked(invoke).mockImplementation((command: string) =>
    command === "get_settings"
      ? Promise.resolve(structuredClone(baseSettings))
      : Promise.resolve(undefined),
  )
})

afterEach(() => {
  vi.useRealTimers()
})

describe("SettingsDialog", () => {
  it("renders the five section tabs in a labelled tablist", async () => {
    render(<SettingsDialog open onOpenChange={vi.fn()} />)

    const tabs = await screen.findAllByRole("tab")

    expect(tabs.map((tab) => tab.textContent)).toEqual([
      "General",
      "Targets",
      "Quality floors",
      "Performance",
      "History",
    ])
    expect(screen.getByRole("tablist")).toHaveAccessibleName(
      "Settings sections",
    )
  })

  it("keeps General to the output folder with an icon-only picker", async () => {
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })

    expect(screen.queryByText("Category targets")).toBeNull()

    const picker = screen.getByRole("button", {
      name: "Choose output folder",
    })
    expect(picker).toHaveTextContent("")
    expect(picker.querySelector("svg")).not.toBeNull()
  })

  it("lists the four target categories with controls and no badges", async () => {
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "Targets" }))

    const formats = await screen.findByText("JPEG, PNG, WebP, HEIC")
    const section = formats.closest('[data-slot="field-set"]') as HTMLElement
    expect(section).not.toBeNull()

    for (const label of ["Images", "Videos", "PDF", "Documents"]) {
      expect(within(section).getByText(label)).toBeInTheDocument()
    }
    expect(section.querySelector('[data-slot="badge"]')).toBeNull()
    for (const pill of ["Image", "Video", "Document"]) {
      expect(within(section).queryByText(pill)).toBeNull()
    }
    expect(within(section).getByLabelText("Images")).toBe(
      within(section).getByLabelText("Images target size"),
    )
    expect(within(section).getByLabelText("Documents")).toBe(
      within(section).getByLabelText("Documents target size"),
    )
  })

  it("saves an edited video target without touching other categories", async () => {
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "Targets" }))
    vi.useFakeTimers()

    fireEvent.change(screen.getByLabelText("Videos target size"), {
      target: { value: "5" },
    })

    await act(async () => {
      vi.advanceTimersByTime(400)
    })

    expect(savedSettings().categoryTargets).toEqual({
      image: { size: 200, unit: "KB" },
      video: { size: 5, unit: "MB" },
      pdf: { size: 1, unit: "MB" },
      document: { size: 2, unit: "MB" },
    })
  })

  it("saves the image long-edge floor without dropping the quality floor", async () => {
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "Quality floors" }))
    const input = screen.getByLabelText("Images minimum long edge")
    expect(input).toHaveDisplayValue("1,280")

    vi.useFakeTimers()
    fireEvent.change(input, { target: { value: "1600" } })

    await act(async () => {
      vi.advanceTimersByTime(400)
    })

    expect(savedSettings().qualityFloors.image).toEqual({
      minQuality: 30,
      minLongEdge: 1600,
    })
  })

  it("saves the full draft once the 400 ms debounce elapses", async () => {
    const input = await openHistoryTab()
    vi.useFakeTimers()

    fireEvent.change(input, { target: { value: "30" } })

    await act(async () => {
      vi.advanceTimersByTime(399)
    })
    expect(saveCalls()).toHaveLength(0)

    await act(async () => {
      vi.advanceTimersByTime(1)
    })

    expect(saveCalls()).toHaveLength(1)
    expect(savedSettings()).toEqual({
      ...baseSettings,
      history: { retentionDays: 30 },
      consent: { allowDestructive: false, heicToJpg: true },
    })

    await act(async () => {
      vi.advanceTimersByTime(400)
    })
    expect(saveCalls()).toHaveLength(1)
  })

  it("coalesces two edits 200 ms apart into one save", async () => {
    const input = await openHistoryTab()
    vi.useFakeTimers()

    fireEvent.change(input, { target: { value: "30" } })
    await act(async () => {
      vi.advanceTimersByTime(200)
    })
    fireEvent.change(input, { target: { value: "31" } })

    await act(async () => {
      vi.advanceTimersByTime(399)
    })
    expect(saveCalls()).toHaveLength(0)

    await act(async () => {
      vi.advanceTimersByTime(1)
    })
    expect(saveCalls()).toHaveLength(1)
    expect(savedSettings().history).toEqual({ retentionDays: 31 })
  })

  it("flushes a pending save exactly once when closed inside the debounce window", async () => {
    const onOpenChange = vi.fn()
    render(<SettingsDialog open onOpenChange={onOpenChange} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()

    fireEvent.change(screen.getByLabelText("History retention (days)"), {
      target: { value: "30" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Done" }))

    await act(async () => {})

    expect(saveCalls()).toHaveLength(1)
    expect(savedSettings()).toEqual({
      ...baseSettings,
      history: { retentionDays: 30 },
      consent: { allowDestructive: false, heicToJpg: true },
    })
    expect(onOpenChange).toHaveBeenCalledWith(false)

    await act(async () => {
      vi.advanceTimersByTime(400)
    })
    expect(saveCalls()).toHaveLength(1)
  })

  it("retries the close flush and saves the draft after a failed write", async () => {
    let saves = 0
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_settings") {
        return Promise.resolve(structuredClone(baseSettings))
      }
      if (command === "save_settings") {
        saves += 1
        return saves === 1
          ? Promise.reject(new Error("disk full"))
          : Promise.resolve(undefined)
      }
      return Promise.resolve(undefined)
    })
    const onOpenChange = vi.fn()
    render(<SettingsDialog open onOpenChange={onOpenChange} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()

    fireEvent.change(screen.getByLabelText("History retention (days)"), {
      target: { value: "30" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Done" }))

    await act(async () => {})

    expect(saveCalls()).toHaveLength(1)
    expect(toast.error).toHaveBeenCalledTimes(1)
    expect(toast.error).toHaveBeenCalledWith(
      "Could not save settings",
      expect.objectContaining({
        id: "settings-save",
        description: "Could not save settings: disk full",
        duration: Infinity,
      }),
    )
    expect(onOpenChange).not.toHaveBeenCalled()
    expect(screen.getByRole("dialog")).toBeInTheDocument()

    fireEvent.click(screen.getByRole("button", { name: "Done" }))
    await act(async () => {})

    expect(saveCalls()).toHaveLength(2)
    expect(savedSettings(1)).toEqual({
      ...baseSettings,
      history: { retentionDays: 30 },
      consent: { allowDestructive: false, heicToJpg: true },
    })
    expect(toast.dismiss).toHaveBeenCalledWith("settings-save")
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("retries a failed debounced save when closed", async () => {
    let saves = 0
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_settings") {
        return Promise.resolve(structuredClone(baseSettings))
      }
      if (command === "save_settings") {
        saves += 1
        return saves === 1
          ? Promise.reject(new Error("disk full"))
          : Promise.resolve(undefined)
      }
      return Promise.resolve(undefined)
    })
    const onOpenChange = vi.fn()
    render(<SettingsDialog open onOpenChange={onOpenChange} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()

    fireEvent.change(screen.getByLabelText("History retention (days)"), {
      target: { value: "30" },
    })
    await act(async () => {
      vi.advanceTimersByTime(400)
    })

    expect(saveCalls()).toHaveLength(1)
    expect(toast.error).toHaveBeenCalledTimes(1)
    expect(onOpenChange).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole("button", { name: "Done" }))
    await act(async () => {})

    expect(saveCalls()).toHaveLength(2)
    expect(savedSettings(1).history).toEqual({ retentionDays: 30 })
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("waits for an in-flight save before closing and retries after it fails", async () => {
    const first = deferred()
    const second = deferred()
    let saves = 0
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_settings") {
        return Promise.resolve(structuredClone(baseSettings))
      }
      if (command === "save_settings") {
        saves += 1
        return saves === 1 ? first.promise : second.promise
      }
      return Promise.resolve(undefined)
    })
    const onOpenChange = vi.fn()
    render(<SettingsDialog open onOpenChange={onOpenChange} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()

    fireEvent.change(screen.getByLabelText("History retention (days)"), {
      target: { value: "30" },
    })
    await act(async () => {
      vi.advanceTimersByTime(400)
    })
    expect(saveCalls()).toHaveLength(1)

    fireEvent.click(screen.getByRole("button", { name: "Done" }))
    await act(async () => {})

    expect(onOpenChange).not.toHaveBeenCalled()

    await act(async () => {
      first.reject(new Error("disk full"))
    })

    expect(toast.error).toHaveBeenCalledWith(
      "Could not save settings",
      expect.objectContaining({ id: "settings-save" }),
    )
    expect(onOpenChange).not.toHaveBeenCalled()
    expect(screen.getByRole("dialog")).toBeInTheDocument()
    expect(saves).toBe(2)

    await act(async () => {
      second.resolve()
    })

    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("saves edits made while a close-flush write is in flight", async () => {
    const first = deferred()
    const second = deferred()
    let saves = 0
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_settings") {
        return Promise.resolve(structuredClone(baseSettings))
      }
      if (command === "save_settings") {
        saves += 1
        return saves === 1 ? first.promise : second.promise
      }
      return Promise.resolve(undefined)
    })
    const onOpenChange = vi.fn()
    render(<SettingsDialog open onOpenChange={onOpenChange} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()
    const input = screen.getByLabelText("History retention (days)")

    fireEvent.change(input, { target: { value: "30" } })
    await act(async () => {
      vi.advanceTimersByTime(400)
    })
    expect(saveCalls()).toHaveLength(1)

    fireEvent.change(input, { target: { value: "31" } })
    fireEvent.click(screen.getByRole("button", { name: "Done" }))
    await act(async () => {
      first.resolve()
    })

    expect(saveCalls()).toHaveLength(2)
    const secondSave = saveCalls()[1]?.[1] as { settings: Settings }
    expect(secondSave.settings.history).toEqual({ retentionDays: 31 })
    expect(onOpenChange).not.toHaveBeenCalled()

    await act(async () => {
      second.resolve()
    })

    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("saves a folder picked with the folder button", async () => {
    vi.mocked(open).mockResolvedValueOnce("/tmp/picked")
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    vi.useFakeTimers()

    fireEvent.click(screen.getByRole("button", { name: "Choose output folder" }))
    await act(async () => {})
    await act(async () => {
      vi.advanceTimersByTime(400)
    })

    expect(saveCalls()).toHaveLength(1)
    expect(savedSettings().outputFolder).toBe("/tmp/picked")
  })

  it("reseeds from loaded settings on reopen and skips saving an untouched draft", async () => {
    vi.mocked(invoke).mockImplementation((command: string) =>
      command === "save_settings"
        ? Promise.reject(new Error("disk full"))
        : Promise.resolve(structuredClone(baseSettings)),
    )
    render(<ForceCloseDialog />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    fireEvent.change(screen.getByLabelText("History retention (days)"), {
      target: { value: "30" },
    })
    fireEvent.blur(screen.getByLabelText("History retention (days)"))

    fireEvent.click(screen.getByText("Force close"))
    await act(async () => {})
    expect(toast.error).toHaveBeenCalled()

    fireEvent.click(screen.getByText("Force open"))
    await act(async () => {})
    expect(screen.getByLabelText("History retention (days)")).toHaveDisplayValue(
      "90",
    )

    vi.mocked(invoke).mockClear()
    fireEvent.click(screen.getByRole("button", { name: "Done" }))
    await act(async () => {})

    expect(saveCalls()).toHaveLength(0)
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    )
  })

  it("keeps the pending edit when a close flush rejects after reopening", async () => {
    const first = deferred()
    let saves = 0
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_settings") {
        return Promise.resolve(structuredClone(baseSettings))
      }
      if (command === "save_settings") {
        saves += 1
        return saves === 1 ? first.promise : Promise.resolve(undefined)
      }
      return Promise.resolve(undefined)
    })
    render(<ForceCloseDialog />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    fireEvent.change(screen.getByLabelText("History retention (days)"), {
      target: { value: "30" },
    })

    fireEvent.click(screen.getByText("Force close"))
    await act(async () => {})
    expect(saveCalls()).toHaveLength(1)

    fireEvent.click(screen.getByText("Force open"))
    await act(async () => {})
    await act(async () => {
      first.reject(new Error("disk full"))
    })
    await act(async () => {})

    expect(screen.getByLabelText("History retention (days)")).toHaveDisplayValue(
      "30",
    )

    fireEvent.click(screen.getByRole("button", { name: "Done" }))
    await act(async () => {})

    expect(saveCalls()).toHaveLength(2)
    expect(savedSettings(1).history).toEqual({ retentionDays: 30 })
  })

  it("toasts when reset fails and keeps the dialog open", async () => {
    vi.mocked(invoke).mockImplementation((command: string) =>
      command === "reset_settings"
        ? Promise.reject(new Error("nope"))
        : Promise.resolve(structuredClone(baseSettings)),
    )
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })

    fireEvent.click(screen.getByRole("button", { name: "Reset defaults" }))
    await act(async () => {})

    expect(toast.error).toHaveBeenCalledTimes(1)
    expect(toast.error).toHaveBeenCalledWith(
      "Could not reset settings",
      expect.objectContaining({
        id: "settings-reset",
        description: "Could not reset settings: nope",
        duration: Infinity,
      }),
    )
    expect(screen.getByRole("dialog")).toBeInTheDocument()
    expect(saveCalls()).toHaveLength(0)
  })

  it("re-arms the draft save after a reset failure", async () => {
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "reset_settings") {
        return Promise.reject(new Error("reset boom"))
      }
      if (command === "save_settings") {
        return Promise.resolve(undefined)
      }
      return Promise.resolve(structuredClone(baseSettings))
    })
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()
    const input = screen.getByLabelText("History retention (days)")

    fireEvent.change(input, { target: { value: "30" } })
    fireEvent.click(screen.getByRole("button", { name: "Reset defaults" }))
    await act(async () => {})

    expect(toast.error).toHaveBeenCalledWith(
      "Could not reset settings",
      expect.objectContaining({ id: "settings-reset" }),
    )
    expect(saveCalls()).toHaveLength(0)

    await act(async () => {
      vi.advanceTimersByTime(400)
    })

    expect(saveCalls()).toHaveLength(1)
    expect(savedSettings().history).toEqual({ retentionDays: 30 })
  })

  it("keeps a failed save's draft when reset races it", async () => {
    const first = deferred()
    let saves = 0
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_settings") {
        return Promise.resolve(structuredClone(baseSettings))
      }
      if (command === "save_settings") {
        saves += 1
        return saves === 1 ? first.promise : Promise.resolve(undefined)
      }
      if (command === "reset_settings") {
        return Promise.reject(new Error("reset boom"))
      }
      return Promise.resolve(undefined)
    })
    const onOpenChange = vi.fn()
    render(<SettingsDialog open onOpenChange={onOpenChange} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()
    const input = screen.getByLabelText("History retention (days)")

    fireEvent.change(input, { target: { value: "30" } })
    await act(async () => {
      vi.advanceTimersByTime(400)
    })
    expect(saveCalls()).toHaveLength(1)

    fireEvent.click(screen.getByRole("button", { name: "Reset defaults" }))
    await act(async () => {})
    await act(async () => {
      first.reject(new Error("disk full"))
    })
    await act(async () => {})

    expect(toast.error).toHaveBeenCalledWith(
      "Could not save settings",
      expect.objectContaining({ id: "settings-save" }),
    )
    expect(toast.error).toHaveBeenCalledWith(
      "Could not reset settings",
      expect.objectContaining({ id: "settings-reset" }),
    )
    const errorMessages = vi
      .mocked(toast.error)
      .mock.calls.map(([message]) => message)
    expect(
      errorMessages.filter((message) => message === "Could not save settings"),
    ).toHaveLength(1)
    expect(
      errorMessages.filter((message) => message === "Could not reset settings"),
    ).toHaveLength(1)

    fireEvent.click(screen.getByRole("button", { name: "Done" }))
    await act(async () => {})

    expect(saveCalls()).toHaveLength(2)
    expect(savedSettings(1).history).toEqual({ retentionDays: 30 })
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("waits for an in-flight save before resetting", async () => {
    const first = deferred()
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_settings") {
        return Promise.resolve(structuredClone(baseSettings))
      }
      if (command === "save_settings") {
        return first.promise
      }
      if (command === "reset_settings") {
        return Promise.resolve(structuredClone(resetDefaults))
      }
      return Promise.resolve(undefined)
    })
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()

    fireEvent.change(screen.getByLabelText("History retention (days)"), {
      target: { value: "31" },
    })
    await act(async () => {
      vi.advanceTimersByTime(400)
    })

    fireEvent.click(screen.getByRole("button", { name: "Reset defaults" }))
    await act(async () => {})
    expect(invoke).not.toHaveBeenCalledWith("reset_settings")

    await act(async () => {
      first.resolve()
    })
    await act(async () => {})

    expect(invoke).toHaveBeenCalledWith("reset_settings")
  })

  it("keeps an edit made while reset is in flight", async () => {
    const resetInFlight = deferred<Settings>()
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_settings") {
        return Promise.resolve(structuredClone(baseSettings))
      }
      if (command === "save_settings") {
        return Promise.resolve(undefined)
      }
      if (command === "reset_settings") {
        return resetInFlight.promise
      }
      return Promise.resolve(undefined)
    })
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()
    const input = screen.getByLabelText("History retention (days)")

    fireEvent.change(input, { target: { value: "30" } })
    fireEvent.click(screen.getByRole("button", { name: "Reset defaults" }))
    await act(async () => {})
    fireEvent.change(input, { target: { value: "31" } })

    await act(async () => {
      resetInFlight.resolve(structuredClone(resetDefaults))
    })
    await act(async () => {})

    expect(input).toHaveDisplayValue("31")

    await act(async () => {
      vi.advanceTimersByTime(400)
    })

    expect(saveCalls()).toHaveLength(1)
    expect(savedSettings().history).toEqual({ retentionDays: 31 })
  })

  it("resets the draft before saving a folder picked afterwards", async () => {
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "reset_settings") {
        return Promise.resolve(structuredClone(resetDefaults))
      }
      return Promise.resolve(structuredClone(baseSettings))
    })
    vi.mocked(open).mockResolvedValueOnce("/tmp/picked")
    const onOpenChange = vi.fn()
    render(<SettingsDialog open onOpenChange={onOpenChange} />)
    await screen.findByRole("tab", { name: "General" })

    fireEvent.click(screen.getByRole("button", { name: "Reset defaults" }))
    await act(async () => {})
    fireEvent.click(screen.getByRole("button", { name: "Choose output folder" }))
    await act(async () => {})
    fireEvent.click(screen.getByRole("button", { name: "Done" }))
    await act(async () => {})

    expect(savedSettings()).toEqual({
      ...resetDefaults,
      outputFolder: "/tmp/picked",
    })
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("toasts instead of closing when the flush write cap is exhausted", async () => {
    const first = deferred()
    const second = deferred()
    const third = deferred()
    let saves = 0
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_settings") {
        return Promise.resolve(structuredClone(baseSettings))
      }
      if (command === "save_settings") {
        saves += 1
        if (saves === 1) return first.promise
        if (saves === 2) return second.promise
        return third.promise
      }
      return Promise.resolve(undefined)
    })
    const onOpenChange = vi.fn()
    render(<SettingsDialog open onOpenChange={onOpenChange} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()
    const input = screen.getByLabelText("History retention (days)")

    fireEvent.change(input, { target: { value: "30" } })
    fireEvent.click(screen.getByRole("button", { name: "Done" }))
    await act(async () => {})
    expect(saveCalls()).toHaveLength(1)

    fireEvent.change(input, { target: { value: "31" } })
    await act(async () => {
      first.resolve()
    })
    expect(saveCalls()).toHaveLength(2)

    fireEvent.change(input, { target: { value: "32" } })
    await act(async () => {
      second.resolve()
    })
    expect(saveCalls()).toHaveLength(3)

    fireEvent.change(input, { target: { value: "33" } })
    await act(async () => {
      third.resolve()
    })

    expect(saveCalls()).toHaveLength(3)
    expect(onOpenChange).not.toHaveBeenCalled()
    expect(toast.error).toHaveBeenCalledTimes(1)
    expect(toast.error).toHaveBeenCalledWith(
      "Settings still not saved — close again to retry",
      expect.objectContaining({
        id: "settings-save",
        duration: Infinity,
      }),
    )
  })

  it("flushes a pending draft on unmount", async () => {
    const view = render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    fireEvent.change(screen.getByLabelText("History retention (days)"), {
      target: { value: "30" },
    })

    view.unmount()
    await act(async () => {})

    expect(saveCalls()).toHaveLength(1)
    expect(savedSettings().history).toEqual({ retentionDays: 30 })
  })

  it("shows the load error instead of the loading text", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("load boom"))
    render(<SettingsDialog open onOpenChange={vi.fn()} />)

    expect(
      await screen.findByText(/Could not load settings/),
    ).toBeInTheDocument()
    expect(screen.queryByText("Loading settings…")).not.toBeInTheDocument()
  })

  it("toasts every consecutive failed save with the same stable id", async () => {
    vi.mocked(invoke).mockImplementation((command: string) =>
      command === "save_settings"
        ? Promise.reject(new Error("disk full"))
        : Promise.resolve(structuredClone(baseSettings)),
    )
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    vi.useFakeTimers()
    const input = screen.getByLabelText("History retention (days)")

    fireEvent.change(input, { target: { value: "30" } })
    await act(async () => {
      vi.advanceTimersByTime(400)
    })
    fireEvent.change(input, { target: { value: "31" } })
    await act(async () => {
      vi.advanceTimersByTime(400)
    })

    expect(saveCalls()).toHaveLength(2)
    const calls = vi.mocked(toast.error).mock.calls
    expect(calls).toHaveLength(2)
    expect(calls[0][0]).toBe("Could not save settings")
    expect(calls[1][0]).toBe("Could not save settings")
    calls.forEach((call) =>
      expect(call[1]).toMatchObject({ id: "settings-save" }),
    )
  })

  it("resets the draft to the returned defaults", async () => {
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "reset_settings") {
        return Promise.resolve(structuredClone(resetDefaults))
      }
      return Promise.resolve(structuredClone(baseSettings))
    })
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "History" }))
    expect(screen.getByLabelText("History retention (days)")).toHaveDisplayValue(
      "90",
    )

    fireEvent.click(screen.getByRole("button", { name: "Reset defaults" }))
    await act(async () => {})

    expect(invoke).toHaveBeenCalledWith("reset_settings")
    expect(toast.dismiss).toHaveBeenCalledWith("settings-save")
    expect(toast.dismiss).toHaveBeenCalledWith("settings-reset")
    expect(screen.getByLabelText("History retention (days)")).toHaveDisplayValue(
      "30",
    )

    fireEvent.click(screen.getByRole("tab", { name: "General" }))
    expect(screen.getByLabelText("Output folder")).toHaveValue(
      "/defaults/Compressed",
    )
  })

  it("offers clear history in the History section behind a destructive confirm", async () => {
    const clearHistory = useHistoryContextMock().clearHistory
    const user = userEvent.setup()
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await user.click(await screen.findByRole("tab", { name: "History" }))
    await user.click(screen.getByRole("button", { name: "Clear history" }))
    await user.click(await screen.findByRole("button", { name: "Delete history" }))
    await waitFor(() => expect(clearHistory).toHaveBeenCalledOnce())
  })

  it("does not clear history when the confirm is dismissed", async () => {
    const clearHistory = useHistoryContextMock().clearHistory
    const user = userEvent.setup()
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await user.click(await screen.findByRole("tab", { name: "History" }))
    await user.click(screen.getByRole("button", { name: "Clear history" }))
    await user.click(await screen.findByRole("button", { name: "Cancel" }))
    expect(clearHistory).not.toHaveBeenCalled()
  })

  it("shows memory stats and the configured ceiling in Performance", async () => {
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_settings") {
        return Promise.resolve({
          ...structuredClone(baseSettings),
          performance: { adaptiveMemory: false, memoryCeilingMb: 512 },
        })
      }
      if (command === "memory_stats") {
        return Promise.resolve({
          totalRamBytes: 16 * 1024 * 1024 * 1024,
          budgetBytes: 512 * 1024 * 1024,
          processRssBytes: 0,
        })
      }
      return Promise.resolve(undefined)
    })
    render(<SettingsDialog open onOpenChange={vi.fn()} />)
    await screen.findByRole("tab", { name: "General" })
    fireEvent.click(screen.getByRole("tab", { name: "Performance" }))

    expect(await screen.findByText("Total RAM")).toBeInTheDocument()
    expect(
      screen.getByLabelText("Memory ceiling in megabytes"),
    ).toHaveDisplayValue("512")
    expect(screen.getByText("16.00 GB")).toBeInTheDocument()
    expect(screen.getByText("512.0 MB")).toBeInTheDocument()
    expect(screen.getByText("—")).toBeInTheDocument()
  })
})
