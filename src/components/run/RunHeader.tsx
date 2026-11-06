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

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import type { BatchCounts, StatusFilter } from '@/lib/grouping'

export interface RunHeaderProps {
  batchNumber: number
  running: boolean
  batchError: string | null
  cancelled?: boolean
  counts: BatchCounts
  total: number
  statusFilter: StatusFilter
  onFilterChange: (f: StatusFilter) => void
  onCancelAll: () => void
  onNewBatch: () => void
}

type ChipCountKey = 'processing' | 'complete' | 'belowTarget' | 'failed' | 'queued'

const FILTER_CHIPS: Array<{
  value: Exclude<StatusFilter, 'all'>
  label: string
  count: ChipCountKey
}> = [
  { value: 'compressing', label: 'Processing', count: 'processing' },
  { value: 'complete', label: 'Complete', count: 'complete' },
  { value: 'below_target', label: 'Below target', count: 'belowTarget' },
  { value: 'failed', label: 'Failed', count: 'failed' },
  { value: 'queued', label: 'Queued', count: 'queued' },
]

const CHIP_BASE =
  'inline-flex cursor-pointer items-center gap-1.5 rounded-full border border-border bg-card px-[11px] py-1 text-[11.5px] font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground'

const CHIP_ACTIVE_PRIMARY =
  'border-primary/35 bg-primary/10 font-semibold text-primary hover:text-primary'

const CHIP_ACTIVE_FAIL =
  'border-destructive/30 bg-destructive/10 font-semibold text-destructive hover:text-destructive'

const CHIP_COUNT = 'font-mono text-[10.5px] font-semibold tabular-nums'

function statusPill(running: boolean, batchError: string | null, cancelled: boolean) {
  if (running) {
    return (
      <span
        data-testid="status-pill"
        data-state="running"
        className="inline-flex items-center gap-1.5 rounded-full border border-primary/30 bg-primary/10 px-[11px] py-1 text-[11px] font-semibold text-primary"
      >
        <span
          aria-hidden
          data-testid="compressing-dot"
          className="size-1.5 animate-pulse rounded-full bg-primary"
        />
        Compressing
      </span>
    )
  }
  if (batchError) {
    return (
      <span
        data-testid="status-pill"
        data-state="failed"
        className="inline-flex items-center gap-1.5 rounded-full border border-destructive/30 bg-destructive/10 px-[11px] py-1 text-[11px] font-semibold text-destructive"
      >
        Failed
      </span>
    )
  }
  if (cancelled) {
    return (
      <span
        data-testid="status-pill"
        data-state="cancelled"
        className="inline-flex items-center gap-1.5 rounded-full border border-border bg-muted px-[11px] py-1 text-[11px] font-semibold text-muted-foreground"
      >
        Cancelled
      </span>
    )
  }
  return (
    <span
      data-testid="status-pill"
      data-state="complete"
      className="inline-flex items-center gap-1.5 rounded-full border border-primary bg-primary px-[11px] py-1 text-[11px] font-semibold text-primary-foreground"
    >
      <svg
        aria-hidden
        width="10"
        height="10"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <polyline points="20 6 9 17 4 12" />
      </svg>
      Complete
    </span>
  )
}

export function RunHeader({
  batchNumber,
  running,
  batchError,
  cancelled = false,
  counts,
  total,
  statusFilter,
  onFilterChange,
  onCancelAll,
  onNewBatch,
}: RunHeaderProps) {
  const progressPercent =
    total > 0 ? Math.min(100, Math.max(0, (counts.processed / total) * 100)) : 0

  return (
    <section data-testid="run-header" className="max-w-full overflow-hidden">
      <div className="flex flex-wrap items-center justify-between gap-3.5 max-w-full overflow-hidden">
        <div className="flex items-center gap-[9px]">
          <span
            data-testid="batch-pill"
            className="rounded-full border border-border bg-card px-3 py-1 font-mono text-xs font-bold tabular-nums text-foreground"
          >
            Batch #{batchNumber}
          </span>
          {statusPill(running, batchError, cancelled)}
          {counts.waiting > 0 && (
            <span
              data-testid="consent-waiting"
              className="inline-flex items-center gap-1.5 rounded-full border border-amber-500/30 bg-amber-500/10 px-[11px] py-1 text-[11px] font-semibold text-amber-600"
            >
              <span
                aria-hidden
                className="size-1.5 rounded-full bg-amber-500"
              />
              {counts.waiting} waiting for permission
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {running ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              data-testid="cancel-all"
              className="border-destructive/30 bg-transparent text-destructive hover:bg-destructive/10 hover:text-destructive"
              onClick={onCancelAll}
            >
              Cancel All
            </Button>
          ) : (
            <Button type="button" size="sm" data-testid="new-batch" onClick={onNewBatch}>
              New batch
            </Button>
          )}
        </div>
      </div>
      {!running && batchError && (
        <p
          data-testid="batch-error"
          role="alert"
          className="mt-2.5 text-[11.5px] leading-[1.45] text-destructive"
        >
          {batchError}
        </p>
      )}
      <div className="mt-3.5 flex flex-wrap items-center gap-[7px]">
        <button
          type="button"
          data-testid="chip-all"
          aria-pressed={statusFilter === 'all'}
          className={cn(CHIP_BASE, statusFilter === 'all' && CHIP_ACTIVE_PRIMARY)}
          onClick={() => onFilterChange('all')}
        >
          All files
          <span data-testid="chip-count" className={CHIP_COUNT}>
            {counts.all}
          </span>
        </button>
        {FILTER_CHIPS.filter((chip) => counts[chip.count] > 0).map((chip) => {
          const active = statusFilter === chip.value
          return (
            <button
              key={chip.value}
              type="button"
              data-testid={`chip-${chip.value}`}
              aria-pressed={active}
              className={cn(
                CHIP_BASE,
                active && (chip.value === 'failed' ? CHIP_ACTIVE_FAIL : CHIP_ACTIVE_PRIMARY)
              )}
              onClick={() => onFilterChange(active ? 'all' : chip.value)}
            >
              {chip.label}
              <span data-testid="chip-count" className={CHIP_COUNT}>
                {counts[chip.count]}
              </span>
            </button>
          )
        })}
        <div data-testid="batch-progress" className="ml-auto flex items-center gap-[9px]">
          <span className="whitespace-nowrap font-mono text-[11px] tabular-nums text-muted-foreground">
            Batch: {counts.processed}/{total} processed
          </span>
          <div className="h-[5px] w-[110px] overflow-hidden rounded-full bg-muted">
            <div
              data-testid="batch-progress-fill"
              className="h-full rounded-full bg-primary"
              style={{ width: `${progressPercent}%` }}
            />
          </div>
        </div>
      </div>
    </section>
  )
}
