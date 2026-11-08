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

import { useState, type ReactElement, type ReactNode } from "react"
import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import { version } from "../../package.json"
import thirdPartyNotices from "../../THIRD_PARTY_LICENSES.md?raw"
import { AboutDialog } from "@/components/AboutDialog"
import {
  UpdateContext,
  type UpdateContextValue,
} from "@/providers/UpdateProvider"

const LICENSE_ROWS = [
  ["Yard", "GPL-3.0"],
  ["FFmpeg / ffprobe", "GPL-2.0-or-later"],
  ["Ghostscript", "AGPL-3.0"],
  ["libvips", "LGPL-3.0"],
  ["qpdf", "Apache-2.0"],
]

afterEach(() => {
  vi.restoreAllMocks()
})

const updateValue: UpdateContextValue = {
  state: { status: "idle" },
  canCheck: true,
  isCompressing: false,
  check: vi.fn(),
  startDownload: vi.fn(),
  install: vi.fn(),
  decline: vi.fn(),
  retry: vi.fn(),
  setSurfaceOpen: vi.fn(),
}

function UpdateWrapper({ children }: { children: ReactNode }) {
  return (
    <UpdateContext.Provider value={updateValue}>{children}</UpdateContext.Provider>
  )
}

function renderAbout(ui: ReactElement) {
  return render(ui, { wrapper: UpdateWrapper })
}

function ControlledDialog() {
  const [open, setOpen] = useState(true)

  return (
    <div>
      <button onClick={() => setOpen(true)}>Open about</button>
      <AboutDialog open={open} onOpenChange={setOpen} />
    </div>
  )
}

describe("AboutDialog", () => {
  it("shows the app identity and the package version", () => {
    renderAbout(<AboutDialog open onOpenChange={vi.fn()} />)

    expect(screen.getByRole("heading", { name: "Yard" })).toBeInTheDocument()
    expect(screen.getByText("Offline file compression tool")).toBeInTheDocument()
    expect(screen.getByText(`Version ${version}`)).toBeInTheDocument()
  })

  it("shows the update check in About", () => {
    renderAbout(<AboutDialog open onOpenChange={vi.fn()} />)

    expect(
      screen.getByRole("button", { name: "Check for updates" }),
    ).toBeInTheDocument()
  })

  it("shows the official app icon", () => {
    renderAbout(<AboutDialog open onOpenChange={vi.fn()} />)

    expect(screen.getByTestId("app-icon")).toHaveAttribute("src", "/favicon.png")
  })

  it("lists the five license rows as name and license pairs", () => {
    renderAbout(<AboutDialog open onOpenChange={vi.fn()} />)

    const dialog = screen.getByRole("dialog")
    const rows = within(dialog).getAllByRole("listitem")

    expect(rows).toHaveLength(5)
    LICENSE_ROWS.forEach(([name, license], index) => {
      expect(within(rows[index]).getByText(name)).toBeInTheDocument()
      expect(within(rows[index]).getByText(license)).toBeInTheDocument()
    })
  })

  it("expands the bundled notices verbatim without a network request", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch")
    renderAbout(<AboutDialog open onOpenChange={vi.fn()} />)
    const user = userEvent.setup()

    expect(thirdPartyNotices.length).toBeGreaterThan(0)
    expect(screen.getByRole("dialog").querySelector("pre")).toBeNull()

    await user.click(screen.getByRole("button", { name: /Third-party notices/ }))

    const notices = screen.getByRole("dialog").querySelector("pre")

    expect(notices?.textContent).toBe(thirdPartyNotices)
    expect(fetchSpy).not.toHaveBeenCalled()
  })

  it("keeps the expanded notices from stretching the dialog", async () => {
    renderAbout(<AboutDialog open onOpenChange={vi.fn()} />)
    const user = userEvent.setup()

    await user.click(screen.getByRole("button", { name: /Third-party notices/ }))

    const notices = screen.getByRole("dialog").querySelector("pre")

    expect(notices).toHaveClass("whitespace-pre-wrap")
    expect(notices).toHaveClass("wrap-break-word")

    const scrollArea = notices?.closest('[data-slot="scroll-area"]')

    expect(scrollArea).toHaveClass("min-w-0")
    expect(scrollArea).toHaveClass("w-full")
  })

  it("ties the rendered license rows to the generated notices", () => {
    renderAbout(<AboutDialog open onOpenChange={vi.fn()} />)

    const rows = within(screen.getByRole("dialog")).getAllByRole("listitem")
    const licenses = rows
      .slice(1)
      .map((row) => row.querySelectorAll("span")[1].textContent ?? "")

    licenses.forEach((license) => {
      expect(thirdPartyNotices).toContain(`License: ${license}`)
    })

    const noticeComponents = licenses.reduce(
      (total, license) =>
        total +
        (thirdPartyNotices.match(new RegExp(`License: ${license}`, "g"))
          ?.length ?? 0),
      0,
    )

    expect(thirdPartyNotices).toContain(`Components: ${noticeComponents}`)
  })

  it("collapses the notices when the toggle is clicked again", async () => {
    renderAbout(<AboutDialog open onOpenChange={vi.fn()} />)
    const user = userEvent.setup()
    const toggle = screen.getByRole("button", { name: /Third-party notices/ })

    expect(toggle).toHaveAttribute("aria-expanded", "false")
    expect(toggle).toHaveAttribute("aria-controls", "third-party-notices")

    await user.click(toggle)

    expect(toggle).toHaveAttribute("aria-expanded", "true")
    expect(document.getElementById("third-party-notices")).not.toBeNull()
    expect(screen.getByRole("dialog").querySelector("pre")).not.toBeNull()

    await user.click(toggle)

    expect(toggle).toHaveAttribute("aria-expanded", "false")
    expect(document.getElementById("third-party-notices")).toBeNull()
    expect(screen.getByRole("dialog").querySelector("pre")).toBeNull()
  })

  it("resets the notices when the parent closes and reopens the dialog", async () => {
    const { rerender } = renderAbout(
      <AboutDialog open onOpenChange={vi.fn()} />,
    )
    const user = userEvent.setup()

    await user.click(screen.getByRole("button", { name: /Third-party notices/ }))
    expect(screen.getByRole("dialog").querySelector("pre")).not.toBeNull()

    rerender(<AboutDialog open={false} onOpenChange={vi.fn()} />)
    rerender(<AboutDialog open onOpenChange={vi.fn()} />)

    expect(screen.getByRole("dialog").querySelector("pre")).toBeNull()
  })

  it("requests a close from the close button", async () => {
    const onOpenChange = vi.fn()
    renderAbout(<AboutDialog open onOpenChange={onOpenChange} />)
    const user = userEvent.setup()

    await user.click(screen.getByRole("button", { name: "Close" }))

    expect(onOpenChange).toHaveBeenCalledTimes(1)
    expect(onOpenChange.mock.calls[0][0]).toBe(false)
  })

  it("requests a close on Escape", async () => {
    const onOpenChange = vi.fn()
    renderAbout(<AboutDialog open onOpenChange={onOpenChange} />)
    const user = userEvent.setup()

    await user.keyboard("{Escape}")

    expect(onOpenChange).toHaveBeenCalledTimes(1)
    expect(onOpenChange.mock.calls[0][0]).toBe(false)
  })

  it("renders no dialog while closed", () => {
    renderAbout(<AboutDialog open={false} onOpenChange={vi.fn()} />)

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    expect(
      screen.queryByRole("heading", { name: "Yard" }),
    ).not.toBeInTheDocument()
  })

  it("starts with the notices collapsed when reopened", async () => {
    renderAbout(<ControlledDialog />)
    const user = userEvent.setup()

    await user.click(screen.getByRole("button", { name: /Third-party notices/ }))
    expect(screen.getByRole("dialog").querySelector("pre")).not.toBeNull()

    await user.click(screen.getByRole("button", { name: "Close" }))
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    )

    await user.click(screen.getByRole("button", { name: "Open about" }))
    expect(
      (await screen.findByRole("dialog")).querySelector("pre"),
    ).toBeNull()
  })
})
