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

import type { ReactElement } from "react"
import type { FileStatus, HistorySession } from "@/types"
import { savedLabel } from "@/lib/historyFormat"
import {
  HISTORY_STATUS_TONES,
  batchSizeSummary,
  outcomePillCounts,
} from "@/lib/historyTable"
import { formatSize } from "@/lib/sizeFormat"

const CARD_CLASS =
  "relative flex min-h-[112px] min-w-0 flex-col rounded-[14px] bg-muted/40 p-[13px_14px] @max-[860px]:min-h-[104px]"
const LABEL_CLASS = "flex items-center gap-[5px] text-[10.5px] text-muted-foreground"
const NUMBER_CLASS =
  "flex flex-1 items-center justify-center text-[27px] leading-none font-[650] tracking-[-0.045em] text-foreground"

function splitSize(bytes: number): { value: string; unit: string | null } {
  const label = formatSize(bytes)
  const space = label.lastIndexOf(" ")
  if (space === -1) return { value: label, unit: null }
  return { value: label.slice(0, space), unit: label.slice(space + 1) }
}

function Pill({
  status,
  label,
  count,
  testId,
}: {
  status: FileStatus
  label: string
  count: number
  testId: string
}) {
  const tone = HISTORY_STATUS_TONES[status]
  return (
    <span
      data-testid={testId}
      className={`flex h-[18px] min-w-0 items-center gap-1 rounded-full border px-1.5 text-[9px] font-semibold ${tone.tint} ${tone.border} ${tone.text}`}
    >
      <i aria-hidden="true" className={`size-[5px] shrink-0 rounded-full ${tone.accent}`} />
      <span className="truncate">{label}</span>
      <b className="ml-auto font-[650] tabular-nums">{count}</b>
    </span>
  )
}

export function HistorySummaryCards({
  session,
}: {
  session: HistorySession
}): ReactElement {
  const files = session.files
  const summary = batchSizeSummary(files)
  const pills = outcomePillCounts(files)
  const currentPercent =
    summary.originalBytes > 0 ? Math.min(100, Math.max(0, 100 - summary.percent)) : 0
  const gonePercent = summary.originalBytes > 0 ? 100 - currentPercent : 0
  const showCurrentLabel = currentPercent > 0 && currentPercent < 100
  const current = splitSize(summary.currentBytes)

  return (
    <div
      data-testid="history-summary-cards"
      className="grid grid-cols-2 gap-3 @min-[860px]:grid-cols-[2.2fr_1.6fr_.9fr_.9fr]"
    >
      <div
        data-testid="summary-size"
        className={`${CARD_CLASS} grid grid-cols-[190px_1fr] items-center gap-2.5 @max-[860px]:grid-cols-[150px_1fr]`}
      >
        <div className="flex min-w-0 flex-col gap-1">
          <div className="text-[8px] uppercase tracking-[0.08em] text-muted-foreground">
            Size
          </div>
          <div className="relative h-3 text-[8px] text-muted-foreground tabular-nums">
            <span className="absolute left-0">0</span>
            {showCurrentLabel && (
              <span
                className="absolute -translate-x-1/2 font-semibold text-foreground"
                style={{ left: `${currentPercent}%` }}
              >
                {formatSize(summary.currentBytes)}
              </span>
            )}
            <span className="absolute right-0 font-semibold text-foreground">
              {formatSize(summary.originalBytes)}
            </span>
          </div>
          <div
            role="img"
            aria-label={`Current size ${formatSize(summary.currentBytes)} of ${formatSize(
              summary.originalBytes
            )} original; ${summary.percent}% saved`}
            className="relative flex h-[7px] rounded-full bg-muted"
            data-testid="summary-scale"
          >
            <span
              aria-hidden="true"
              className="h-full rounded-l-full bg-primary"
              style={{ width: `${currentPercent}%` }}
              data-testid="summary-current-bar"
            />
            <span
              aria-hidden="true"
              className="h-full rounded-r-full bg-muted-foreground/20"
              style={{ width: `${gonePercent}%` }}
            />
            {currentPercent > 0 && currentPercent < 100 && (
              <span
                aria-hidden="true"
                className="absolute -top-[3px] -bottom-[3px] w-0.5 rounded-full bg-foreground"
                style={{ left: `${currentPercent}%` }}
              />
            )}
          </div>
          <div className="text-[9px] font-[650] text-primary">{summary.percent}% saved</div>
        </div>

        <div className="min-w-0 text-right">
          <div
            data-testid="summary-current"
            className="text-[24px] leading-none font-[650] tracking-[-0.04em] @max-[860px]:text-[20px]"
          >
            {current.value}
            {current.unit !== null && (
              <>
                {" "}
                <small className="text-[11px] font-medium tracking-normal text-muted-foreground">
                  {current.unit}
                </small>
              </>
            )}
          </div>
          <div className="mt-0.5 text-[7px] uppercase tracking-[0.08em] text-muted-foreground">
            Current
          </div>
          <div className="mt-1 text-[9px] whitespace-nowrap text-muted-foreground">
            {savedLabel(summary.savedBytes)} saved
          </div>
        </div>
      </div>

      <div data-testid="summary-files" className={CARD_CLASS}>
        <div className={LABEL_CLASS}>Files</div>
        <div className={NUMBER_CLASS}>{files.length}</div>
        <div className="mt-[5px] grid grid-cols-2 gap-1">
          <Pill status="done" label="Done" count={pills.done} testId="summary-pill-done" />
          <Pill
            status="already_under_target"
            label="Under target"
            count={pills.underTarget}
            testId="summary-pill-under"
          />
          <Pill status="best_effort" label="Best" count={pills.bestEffort} testId="summary-pill-best" />
          <Pill status="failed" label="Failed" count={pills.failed} testId="summary-pill-failed" />
          {pills.needsPermission > 0 && (
            <Pill
              status="needs_permission"
              label="Permission"
              count={pills.needsPermission}
              testId="summary-pill-permission"
            />
          )}
          {pills.other > 0 && (
            <Pill
              status="cancelled"
              label="Other"
              count={pills.other}
              testId="summary-pill-other"
            />
          )}
        </div>
      </div>

      <div data-testid="summary-best-effort" className={CARD_CLASS}>
        <div className={LABEL_CLASS}>
          <span aria-hidden="true" className="size-[7px] rounded-full bg-amber-500" />
          Best effort
        </div>
        <div className={NUMBER_CLASS}>{pills.bestEffort}</div>
      </div>

      <div data-testid="summary-failed" className={CARD_CLASS}>
        <div className={LABEL_CLASS}>
          <span aria-hidden="true" className="size-[7px] rounded-full bg-destructive" />
          Failed
        </div>
        <div className={NUMBER_CLASS}>{pills.failed}</div>
      </div>
    </div>
  )
}
