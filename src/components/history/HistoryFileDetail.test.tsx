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

import { describe, expect, it, vi } from "vitest"
import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import type { HistoryFile } from "@/types"
import { HistoryFileDetail } from "@/components/history/HistoryFileDetail"

const KB = 1024
const MB = 1024 * 1024

function file(overrides: Partial<HistoryFile> = {}): HistoryFile {
  return {
    name: "3d-layout.png",
    path: "/in/3d-layout.png",
    outputPath: "/out/3d-layout.png",
    category: "image",
    targetBytes: 200 * KB,
    originalSize: 1.8 * MB,
    compressedSize: 259 * KB,
    status: "best_effort",
    qualityUsed: null,
    strategyUsed: "palette",
    error: null,
    durationMs: 2_100,
    completedAt: new Date(2026, 9, 25, 20, 2).getTime(),
    changes: [],
    consentSource: null,
    ...overrides,
  }
}

describe("HistoryFileDetail", () => {
  it("renders the size scale with current/original/percent/saved values", () => {    render(<HistoryFileDetail file={file()} onReveal={vi.fn()} />)

    expect(screen.getByText("0")).toBeInTheDocument()
    expect(screen.getByText("259 KB")).toBeInTheDocument()
    expect(screen.getByText("1.8 MB")).toBeInTheDocument()
    expect(screen.getByText("86% saved")).toBeInTheDocument()
    expect(screen.getByText("1.5 MB saved")).toBeInTheDocument()
    expect(
      screen.getByRole("img", { name: "Current size 259 KB of 1.8 MB original; 86% saved" })
    ).toBeInTheDocument()
    expect(screen.getByTestId("detail-current-bar")).toHaveStyle({ width: "14%" })
  })

  it("renders target/method and timing cards without repeating the file name or status", () => {
    render(<HistoryFileDetail file={file()} onReveal={vi.fn()} />)

    expect(screen.getByText("Target & method")).toBeInTheDocument()
    expect(screen.getByText("Target")).toBeInTheDocument()
    expect(screen.getByText("200 KB")).toBeInTheDocument()
    expect(screen.getByText("Method")).toBeInTheDocument()
    expect(screen.getByText("PNG palette · Lossy")).toBeInTheDocument()
    expect(screen.getByText("Timing")).toBeInTheDocument()
    expect(screen.getByText("Duration")).toBeInTheDocument()
    expect(screen.getByText("2.1 s")).toBeInTheDocument()
    expect(screen.getByText("Completed")).toBeInTheDocument()
    expect(screen.getByText("Oct 25 · 8:02 PM")).toBeInTheDocument()
    expect(screen.queryByText("3d-layout.png")).not.toBeInTheDocument()
    expect(screen.queryByText("Best effort")).not.toBeInTheDocument()
  })

  it("reveals original and compressed from footer actions", async () => {
    const user = userEvent.setup()
    const onReveal = vi.fn()
    render(<HistoryFileDetail file={file()} onReveal={onReveal} />)

    await user.click(screen.getByRole("button", { name: "Show original" }))
    await user.click(screen.getByRole("button", { name: "Show compressed" }))

    expect(onReveal).toHaveBeenNthCalledWith(1, "/in/3d-layout.png")
    expect(onReveal).toHaveBeenNthCalledWith(2, "/out/3d-layout.png")
  })

  it("disables compressed action when outputPath is null", () => {
    render(
      <HistoryFileDetail
        file={file({ status: "failed", compressedSize: null, outputPath: null, error: "locked" })}
        onReveal={vi.fn()}
      />
    )

    expect(screen.getByRole("button", { name: "Show original" })).toBeEnabled()
    expect(screen.getByRole("button", { name: "Show compressed" })).toBeDisabled()
  })

  it("replaces the size card with the full error for a failed file", () => {
    render(
      <HistoryFileDetail
        file={file({
          status: "failed",
          compressedSize: null,
          outputPath: null,
          error: "File is locked by another app",
        })}
        onReveal={vi.fn()}
      />
    )

    expect(screen.getByText("File is locked by another app")).toBeInTheDocument()
    expect(screen.queryByRole("img")).not.toBeInTheDocument()
    expect(screen.getByText("Target & method")).toBeInTheDocument()
    expect(screen.getByText("Timing")).toBeInTheDocument()
  })

  it("shows fallback error copy for a no-output file with no error", () => {
    render(
      <HistoryFileDetail
        file={file({ status: "unsupported", compressedSize: null, outputPath: null, error: null })}
        onReveal={vi.fn()}
      />
    )

    expect(screen.getByText("No output file.")).toBeInTheDocument()
    expect(screen.queryByRole("img")).not.toBeInTheDocument()
  })

  it("shows 0% and current=original for an under-target file", () => {
    render(
      <HistoryFileDetail
        file={file({
          status: "already_under_target",
          originalSize: 950 * KB,
          compressedSize: 950 * KB,
          strategyUsed: null,
        })}
        onReveal={vi.fn()}
      />
    )

    expect(screen.getByText("0% saved")).toBeInTheDocument()
    expect(screen.getAllByText("950 KB")).toHaveLength(1)
    expect(screen.getByTestId("detail-current-bar")).toHaveStyle({ width: "100%" })
    expect(screen.queryByText("NaN")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Show compressed" })).toBeEnabled()
  })

  it("shows the granted changes and the consent source", () => {
    render(
      <HistoryFileDetail
        file={file({
          changes: ["downscale", "drop_stream"],
          consentSource: "batch",
        })}
        onReveal={vi.fn()}
      />
    )

    const card = screen.getByTestId("history-detail-permission-card")
    expect(card).toHaveTextContent("Permissions")
    expect(card).toHaveTextContent("Changes")
    expect(card).toHaveTextContent("Downscale, Drop stream")
    expect(card).toHaveTextContent("Consent")
    expect(card).toHaveTextContent("This batch")
  })

  it("omits the permission card when no changes or consent were recorded", () => {
    const onReveal = vi.fn()
    const { rerender } = render(<HistoryFileDetail file={file()} onReveal={onReveal} />)
    expect(screen.queryByTestId("history-detail-permission-card")).not.toBeInTheDocument()

    rerender(
      <HistoryFileDetail file={file({ consentSource: "default" })} onReveal={onReveal} />
    )
    expect(screen.queryByTestId("history-detail-permission-card")).not.toBeInTheDocument()
  })
})
