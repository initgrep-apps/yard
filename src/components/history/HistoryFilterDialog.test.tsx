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
import { fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { startOfDay, startOfMonth, subDays, subMonths } from "date-fns"
import { HistoryFilterDialog } from "@/components/history/HistoryFilterDialog"
import type { Category, HistoryFilters } from "@/types"

const NEWEST: HistoryFilters = { sort: "newest" }
const NOW = new Date(2026, 9, 23, 14, 30)

const mainDialog = () => screen.getByRole("dialog", { name: "Filter history" })
const dialogButton = (name: string) =>
  within(mainDialog()).getByRole("button", { name })
const button = dialogButton

const sinceTrigger = () =>
  mainDialog().querySelector<HTMLButtonElement>('[data-slot="popover-trigger"]')!

const popoverElement = () => {
  const content = document.querySelector(
    '[data-slot="popover-content"][data-open]',
  )
  if (!content) throw new Error("date popover is not open")
  return content as HTMLElement
}

const popoverContent = () => within(popoverElement())

const popoverIsClosed = () =>
  document.querySelector('[data-slot="popover-content"][data-open]') === null

const openSince = () => {
  fireEvent.click(sinceTrigger())
  return popoverContent()
}

const pressedPresetNames = () =>
  popoverContent()
    .getAllByRole("button")
    .filter((item) => item.getAttribute("aria-pressed") === "true")
    .map((item) => item.textContent)

const clickDay = (iso: string) => {
  const cells = document.querySelectorAll(`[data-day="${iso}"]`)
  const cell = cells[cells.length - 1]
  if (!cell) throw new Error(`day ${iso} is not rendered`)
  fireEvent.click(cell.querySelector("button")!)
}

const setNow = (now = NOW) => {
  vi.useFakeTimers({ toFake: ["Date"] })
  vi.setSystemTime(now)
}

function renderDialog(applied: HistoryFilters = NEWEST) {
  const onApply = vi.fn()
  const onOpenChange = vi.fn()
  const view = render(
    <HistoryFilterDialog
      open
      onOpenChange={onOpenChange}
      applied={applied}
      onApply={onApply}
    />,
  )
  return { ...view, onApply, onOpenChange }
}

function ControlledDialog({
  initial = NEWEST,
  onApply,
}: {
  initial?: HistoryFilters
  onApply: (next: HistoryFilters) => void
}) {
  const [open, setOpen] = useState(true)
  const [applied, setApplied] = useState(initial)

  return (
    <>
      <button onClick={() => setOpen(true)}>Open filters</button>
      <HistoryFilterDialog
        open={open}
        onOpenChange={setOpen}
        applied={applied}
        onApply={(next) => {
          setApplied(next)
          onApply(next)
        }}
      />
    </>
  )
}

afterEach(() => {
  vi.useRealTimers()
})

describe("HistoryFilterDialog", () => {
  it("renders exactly two sort segments and five category chips", () => {
    renderDialog()

    const sortGroup = mainDialog().querySelector(
      '[data-slot="toggle-group"][aria-label="Sort by"]',
    )
    const categoryGroup = mainDialog().querySelector(
      '[data-slot="toggle-group"][aria-label="Category"]',
    )

    expect(within(sortGroup as HTMLElement).getAllByRole("button")).toHaveLength(
      2,
    )
    expect(
      within(categoryGroup as HTMLElement).getAllByRole("button"),
    ).toHaveLength(5)
    expect(button("Newest first")).toHaveAttribute("aria-pressed", "true")
    expect(button("Oldest first")).toHaveAttribute("aria-pressed", "false")
  })

  it("uses a flex column layout with compact legends", () => {
    renderDialog()

    expect(mainDialog()).toHaveClass("flex", "flex-col")
    expect(mainDialog()).not.toHaveClass("grid")
    const legend = mainDialog().querySelector('[data-slot="field-legend"]')
    expect(legend).toHaveClass("data-[variant=legend]:text-xs")
    expect(legend?.className).not.toContain("data-[variant=legend]:text-base")
  })

  it("keeps a sort set when the pressed segment is clicked again", () => {
    renderDialog({ sort: "oldest" })

    fireEvent.click(button("Oldest first"))

    expect(button("Oldest first")).toHaveAttribute("aria-pressed", "true")
  })

  it("toggles statuses independently and counts the active filters", () => {
    const { onApply } = renderDialog()

    fireEvent.click(button("Cancelled"))
    expect(button("Cancelled")).toHaveAttribute("aria-pressed", "true")
    expect(button("Done")).toHaveAttribute("aria-pressed", "false")

    fireEvent.click(button("Done"))
    expect(button("Done")).toHaveAttribute("aria-pressed", "true")
    expect(screen.getByText("2 active")).toBeInTheDocument()

    fireEvent.click(button("Apply"))
    const payload = onApply.mock.calls[0][0]
    expect(payload.statuses).toEqual(
      expect.arrayContaining(["cancelled", "done"]),
    )
    expect(payload.statuses).toHaveLength(2)
  })

  it("renders seven status chips including the three terminal outcomes", () => {
    renderDialog()

    const statusGroup = mainDialog().querySelector(
      '[data-slot="toggle-group"][aria-label="Status"]',
    )

    expect(
      within(statusGroup as HTMLElement).getAllByRole("button"),
    ).toHaveLength(7)
    expect(button("Best effort")).toBeInTheDocument()
    expect(button("Under target")).toBeInTheDocument()
    expect(button("Unsupported")).toBeInTheDocument()
    expect(button("Needs permission")).toBeInTheDocument()
  })

  it("uses the History muted dot for Under target", () => {
    renderDialog()

    const chip = button("Under target")
    expect(chip.querySelector(".bg-muted-foreground")).not.toBeNull()
    expect(chip.querySelector(".bg-primary")).toBeNull()
  })

  it("omits the active badge for filters this dialog cannot show", () => {
    renderDialog({ search: "alpha" })

    expect(screen.queryByText("1 active")).not.toBeInTheDocument()
    expect(screen.queryByText(/active/)).not.toBeInTheDocument()
  })

  it("omits the active badge for an output folder only", () => {
    renderDialog({ outputFolder: "/tmp/out" })

    expect(screen.queryByText(/active/)).not.toBeInTheDocument()
  })

  it("counts an Image-only selection in the badge", () => {
    renderDialog()

    fireEvent.click(button("Image"))

    expect(screen.getByText("1 active")).toBeInTheDocument()
  })

  it("counts Oldest-only in the badge", () => {
    renderDialog({ sort: "oldest" })

    expect(screen.getByText("1 active")).toBeInTheDocument()
  })

  it("renders five category chips with the shared glyphs", () => {
    renderDialog()

    const categories = [
      ["Image", "image"],
      ["Video", "video"],
      ["PDF", "pdf"],
      ["Document", "document"],
      ["Other", "other"],
    ] as const

    for (const [label, category] of categories) {
      const chip = button(label)
      expect(chip.querySelector(`[data-category="${category}"]`)).not.toBeNull()
    }
  })

  it("marks the pressed chips with a visible selected style", () => {
    renderDialog()

    fireEvent.click(button("PDF"))

    const chip = button("PDF")
    expect(chip).toHaveAttribute("aria-pressed", "true")
    expect(chip).toHaveAttribute("data-pressed")
    expect(chip.className).toContain("data-[pressed]:bg-primary/15")
    expect(chip.className).toContain("data-[pressed]:border-primary/50")

    fireEvent.click(button("Failed"))
    const status = button("Failed")
    expect(status).toHaveAttribute("data-pressed")
    expect(status.className).toContain("data-[pressed]:bg-primary/15")
  })

  it("toggles Image and Other", () => {
    const { onApply } = renderDialog()

    fireEvent.click(button("Image"))
    fireEvent.click(button("Other"))

    expect(button("Image")).toHaveAttribute("aria-pressed", "true")
    expect(button("Other")).toHaveAttribute("aria-pressed", "true")
    expect(button("Video")).toHaveAttribute("aria-pressed", "false")

    fireEvent.click(button("Apply"))
    const payload = onApply.mock.calls[0][0]
    expect(payload.categories).toEqual(
      expect.arrayContaining(["image", "other"]),
    )
    expect(payload.categories).toHaveLength(2)
  })

  it("seeds every non-default applied filter on open", () => {
    setNow()

    renderDialog({
      sort: "oldest",
      statuses: ["failed"],
      categories: ["pdf"],
      sinceMs: startOfDay(subDays(NOW, 6)).getTime(),
    })

    expect(button("Oldest first")).toHaveAttribute("aria-pressed", "true")
    expect(button("Newest first")).toHaveAttribute("aria-pressed", "false")
    expect(button("Failed")).toHaveAttribute("aria-pressed", "true")
    expect(button("PDF")).toHaveAttribute("aria-pressed", "true")
    expect(button("Last 7 days")).toBeInTheDocument()
  })

  it("drops seeded statuses and categories that have no chip", () => {
    const { onApply } = renderDialog({
      statuses: ["pending", "compressing", "done"],
      categories: ["image"],
    })

    expect(button("Done")).toHaveAttribute("aria-pressed", "true")
    expect(button("Image")).toHaveAttribute("aria-pressed", "true")

    fireEvent.click(button("Apply"))
    expect(onApply.mock.calls[0][0].statuses).toEqual(["done"])
    expect(onApply.mock.calls[0][0].categories).toEqual(["image"])
  })

  it("drops unknown seeded categories", () => {
    const { onApply } = renderDialog({
      categories: ["image", "archive" as Category],
    })

    fireEvent.click(button("Apply"))
    expect(onApply.mock.calls[0][0].categories).toEqual(["image"])
  })

  it("emits undefined when the seed only has transient statuses", () => {
    const { onApply } = renderDialog({ statuses: ["pending"] })

    fireEvent.click(button("Apply"))
    expect(onApply.mock.calls[0][0].statuses).toBeUndefined()
  })

  it("emits undefined when the last status chip is deselected", () => {
    const { onApply } = renderDialog()

    fireEvent.click(button("Done"))
    fireEvent.click(button("Done"))
    fireEvent.click(button("Apply"))

    expect(onApply.mock.calls[0][0].statuses).toBeUndefined()
  })

  it("emits undefined when the last category chip is deselected", () => {
    const { onApply } = renderDialog()

    fireEvent.click(button("Image"))
    fireEvent.click(button("Image"))
    fireEvent.click(button("Apply"))

    expect(onApply.mock.calls[0][0].categories).toBeUndefined()
  })

  it("uses the live applied hidden fields at apply time", () => {
    const { onApply, onOpenChange, rerender } = renderDialog({
      search: "alpha",
    })

    fireEvent.click(button("Image"))
    rerender(
      <HistoryFilterDialog
        open
        onOpenChange={onOpenChange}
        applied={{ search: "beta" }}
        onApply={onApply}
      />,
    )
    fireEvent.click(button("Apply"))

    expect(onApply.mock.calls[0][0].search).toBe("beta")
    expect(onApply.mock.calls[0][0].categories).toEqual(["image"])
  })

  it("keeps search and outputFolder through the draft", () => {
    const { onApply } = renderDialog({
      search: "report",
      outputFolder: "/tmp/out",
    })

    fireEvent.click(button("Done"))
    fireEvent.click(button("Apply"))

    expect(onApply).toHaveBeenCalledWith(
      expect.objectContaining({
        search: "report",
        outputFolder: "/tmp/out",
        statuses: ["done"],
      }),
    )
  })

  it("discards a chip change on Cancel and reseeds on reopen", () => {
    render(
      <ControlledDialog
        initial={{ statuses: ["failed"] }}
        onApply={vi.fn()}
      />,
    )

    fireEvent.click(button("Done"))
    fireEvent.click(button("Cancel"))

    fireEvent.click(screen.getByRole("button", { name: "Open filters" }))

    expect(button("Done")).toHaveAttribute("aria-pressed", "false")
    expect(button("Failed")).toHaveAttribute("aria-pressed", "true")
  })

  it.each([
    ["Today", (now: Date) => startOfDay(now)],
    ["Last 7 days", (now: Date) => startOfDay(subDays(now, 6))],
    ["Last 30 days", (now: Date) => startOfDay(subDays(now, 29))],
    ["This month", (now: Date) => startOfMonth(now)],
    ["Last 3 months", (now: Date) => startOfDay(subMonths(now, 3))],
  ])("maps %s to the start of its range", (label, start) => {
    setNow()
    const { onApply } = renderDialog()

    const popover = openSince()
    fireEvent.click(popover.getByRole("button", { name: label }))

    expect(popoverIsClosed()).toBe(true)
    fireEvent.click(button("Apply"))

    expect(onApply).toHaveBeenCalledWith(
      expect.objectContaining({ sinceMs: start(NOW).getTime() }),
    )
  })

  it("marks only the resolved preset on a colliding date", () => {
    setNow(new Date(2026, 9, 1, 10, 0))
    renderDialog({ sinceMs: new Date(2026, 9, 1).getTime() })

    openSince()

    expect(pressedPresetNames()).toEqual(["Today"])
    expect(button("Today")).toBeInTheDocument()
    expect(
      popoverContent().getByRole("button", { name: "All time" }),
    ).toHaveAttribute("aria-pressed", "false")
  })

  it("marks only Last 7 days when it collides with This month", () => {
    setNow(new Date(2026, 9, 7, 10, 0))
    renderDialog({ sinceMs: new Date(2026, 9, 1).getTime() })

    openSince()

    expect(pressedPresetNames()).toEqual(["Last 7 days"])
  })

  it("marks All time on a fresh popover and not Today", () => {
    setNow()
    renderDialog()

    const popover = openSince()

    expect(popover.getByRole("button", { name: "All time" })).toHaveAttribute(
      "aria-pressed",
      "true",
    )
    expect(popover.getByRole("button", { name: "Today" })).toHaveAttribute(
      "aria-pressed",
      "false",
    )
  })

  it("closes the date popover when the dialog is closed and reopened", () => {
    setNow()
    const { onApply, onOpenChange, rerender } = renderDialog()

    openSince()
    expect(popoverIsClosed()).toBe(false)

    rerender(
      <HistoryFilterDialog
        open={false}
        onOpenChange={onOpenChange}
        applied={NEWEST}
        onApply={onApply}
      />,
    )
    rerender(
      <HistoryFilterDialog
        open
        onOpenChange={onOpenChange}
        applied={NEWEST}
        onApply={onApply}
      />,
    )

    expect(popoverIsClosed()).toBe(true)
  })

  it("labels the trigger and counts the badge after a preset", () => {
    setNow()
    renderDialog()

    const popover = openSince()
    fireEvent.click(popover.getByRole("button", { name: "This month" }))

    expect(popoverIsClosed()).toBe(true)
    expect(button("This month")).toBeInTheDocument()
    expect(screen.getByText("1 active")).toBeInTheDocument()
  })

  it("maps a custom calendar date to that day's start", () => {
    setNow()
    const { onApply } = renderDialog()

    openSince()
    clickDay("2026-10-16")

    expect(popoverIsClosed()).toBe(true)
    fireEvent.click(button("Apply"))
    expect(onApply).toHaveBeenCalledWith(
      expect.objectContaining({
        sinceMs: new Date(2026, 9, 16).getTime(),
      }),
    )
  })

  it("opens a previous-month custom date on its month and selects it", () => {
    setNow()
    const sinceMs = new Date(2026, 8, 15).getTime()
    renderDialog({ sinceMs })

    openSince()

    const day = document.querySelector('[data-day="2026-09-15"] button')
    expect(day).not.toBeNull()
    expect(day).toHaveAttribute("data-selected-single", "true")
  })

  it("clears the date when the selected custom day is clicked again", () => {
    setNow()
    const { onApply } = renderDialog()

    openSince()
    clickDay("2026-10-16")
    expect(popoverIsClosed()).toBe(true)

    openSince()
    clickDay("2026-10-16")
    expect(popoverIsClosed()).toBe(true)

    expect(button("All time")).toBeInTheDocument()
    fireEvent.click(button("Apply"))
    expect(onApply).toHaveBeenCalledWith(
      expect.objectContaining({ sinceMs: undefined }),
    )
  })

  it("disables future days but not today", () => {
    setNow()
    renderDialog()

    openSince()

    expect(
      document.querySelector('[data-day="2026-10-23"] button'),
    ).not.toBeDisabled()
    expect(
      document.querySelector('[data-day="2026-10-24"] button'),
    ).toBeDisabled()
  })

  it("reopens the popover and clears the date with All time", () => {
    setNow()
    const onApply = vi.fn()
    render(<ControlledDialog onApply={onApply} />)

    const popover = openSince()
    fireEvent.click(popover.getByRole("button", { name: "Last 7 days" }))
    fireEvent.click(button("Apply"))

    fireEvent.click(screen.getByRole("button", { name: "Open filters" }))
    expect(popoverIsClosed()).toBe(true)
    expect(button("Last 7 days")).toBeInTheDocument()

    const reopened = openSince()
    expect(reopened.getByRole("button", { name: "Last 7 days" })).toHaveAttribute(
      "aria-pressed",
      "true",
    )
    fireEvent.click(reopened.getByRole("button", { name: "All time" }))
    fireEvent.click(button("Apply"))

    expect(onApply).toHaveBeenLastCalledWith(
      expect.objectContaining({ sinceMs: undefined }),
    )
  })

  it("applies the draft and closes", () => {
    const { onApply, onOpenChange } = renderDialog()

    fireEvent.click(button("Oldest first"))
    fireEvent.click(button("Done"))
    fireEvent.click(button("Apply"))

    expect(onApply).toHaveBeenCalledWith(
      expect.objectContaining({ sort: "oldest", statuses: ["done"] }),
    )
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("closes from the title-row close button", () => {
    const { onApply, onOpenChange } = renderDialog()

    fireEvent.click(button("Close"))

    expect(onApply).not.toHaveBeenCalled()
    expect(onOpenChange.mock.calls[0][0]).toBe(false)
  })

  it("discards the draft on Cancel", () => {
    const { onApply, onOpenChange } = renderDialog()

    fireEvent.click(button("Done"))
    fireEvent.click(button("Cancel"))

    expect(onApply).not.toHaveBeenCalled()
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("clears only the draft on Clear all", () => {
    setNow()
    const { onApply, onOpenChange } = renderDialog({
      sort: "oldest",
      statuses: ["done"],
      categories: ["image"],
      sinceMs: startOfDay(subDays(NOW, 6)).getTime(),
    })

    fireEvent.click(button("Clear all"))

    expect(button("Newest first")).toHaveAttribute("aria-pressed", "true")
    expect(button("Oldest first")).toHaveAttribute("aria-pressed", "false")
    expect(button("Done")).toHaveAttribute("aria-pressed", "false")
    expect(button("Image")).toHaveAttribute("aria-pressed", "false")
    expect(button("All time")).toBeInTheDocument()
    expect(onApply).not.toHaveBeenCalled()
    expect(onOpenChange).not.toHaveBeenCalled()

    fireEvent.click(button("Apply"))
    expect(onApply).toHaveBeenCalledWith({
      search: undefined,
      outputFolder: undefined,
      statuses: undefined,
      categories: undefined,
      sinceMs: undefined,
      sort: "newest",
    })
  })

  it("seeds the draft from the applied filters on reopen", () => {
    const onApply = vi.fn()
    render(<ControlledDialog onApply={onApply} />)

    fireEvent.click(button("Oldest first"))
    fireEvent.click(button("Image"))
    fireEvent.click(button("Apply"))

    fireEvent.click(screen.getByRole("button", { name: "Open filters" }))

    expect(button("Oldest first")).toHaveAttribute("aria-pressed", "true")
    expect(button("Newest first")).toHaveAttribute("aria-pressed", "false")
    expect(button("Image")).toHaveAttribute("aria-pressed", "true")
    expect(button("Video")).toHaveAttribute("aria-pressed", "false")
  })

  it("normalizes an unknown sort and a non-finite since", () => {
    const { onApply } = renderDialog({
      sort: "asc" as unknown as HistoryFilters["sort"],
      sinceMs: Number.NaN,
    })

    expect(button("Newest first")).toHaveAttribute("aria-pressed", "true")
    expect(button("All time")).toBeInTheDocument()

    fireEvent.click(button("Apply"))
    expect(onApply).toHaveBeenCalledWith(
      expect.objectContaining({ sort: "newest", sinceMs: undefined }),
    )
  })

  it("does not reset the draft when applied changes identity while open", () => {
    const { onApply, rerender } = renderDialog()

    fireEvent.click(button("Done"))
    rerender(
      <HistoryFilterDialog
        open
        onOpenChange={vi.fn()}
        applied={{ sort: "oldest" }}
        onApply={onApply}
      />,
    )

    expect(button("Done")).toHaveAttribute("aria-pressed", "true")
    expect(button("Newest first")).toHaveAttribute("aria-pressed", "true")
    expect(button("Oldest first")).toHaveAttribute("aria-pressed", "false")
  })
})
