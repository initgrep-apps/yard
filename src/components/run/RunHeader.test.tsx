import { beforeAll, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { RunHeader } from '@/components/run/RunHeader'
import { setupMotionStubs } from '@/test/motionStubs'
import { batchCounts, type BatchCounts } from '@/lib/grouping'
import type { FileEntry } from '@/types'

beforeAll(() => {
  setupMotionStubs()
})

let entrySeq = 0
function entry(status: FileEntry['status']): FileEntry {
  entrySeq += 1
  return {
    id: `entry-${entrySeq}`,
    name: 'f',
    path: '/tmp/f',
    category: 'video',
    targetBytes: 1,
    originalSize: 1,
    status,
    compressedSize: null,
    outputPath: null,
    error: null,
    strategyUsed: null,
    qualityUsed: null,
  }
}

function countsOf(statuses: Array<FileEntry['status']>): BatchCounts {
  return batchCounts(statuses.map(entry))
}

function baseProps() {
  return {
    batchNumber: 12,
    running: false,
    batchError: null,
    total: 10,
    statusFilter: 'all' as const,
    onFilterChange: vi.fn(),
    onCancelAll: vi.fn(),
    onNewBatch: vi.fn(),
  }
}

describe('RunHeader', () => {
  it('renders Batch #n, status pill per running/complete/error state, and error line on batchError', () => {
    const { rerender } = render(
      <RunHeader {...baseProps()} running counts={countsOf(['compressing', 'compressing'])} />
    )
    expect(screen.getByTestId('batch-pill').textContent).toBe('Batch #12')
    const runningPill = screen.getByTestId('status-pill')
    expect(runningPill.textContent).toBe('Compressing')
    expect(runningPill.className).toContain('text-primary')
    expect(screen.getByTestId('compressing-dot').className).toContain('animate-pulse')

    rerender(<RunHeader {...baseProps()} counts={countsOf(['done', 'failed'])} />)
    const completePill = screen.getByTestId('status-pill')
    expect(completePill.textContent).toBe('Complete')
    expect(completePill.className).toContain('bg-primary')
    expect(completePill.className).toContain('text-primary-foreground')
    expect(screen.queryByTestId('compressing-dot')).not.toBeInTheDocument()

    rerender(
      <RunHeader
        {...baseProps()}
        batchError="Ghostscript worker unavailable"
        counts={countsOf(['done'])}
      />
    )
    const failedPill = screen.getByTestId('status-pill')
    expect(failedPill.textContent).toBe('Failed')
    expect(failedPill.className).toContain('text-destructive')
    expect(screen.getByTestId('batch-error').textContent).toBe('Ghostscript worker unavailable')
  })

  it('a persisted batch error does not masquerade as live-run state while running', () => {
    render(
      <RunHeader
        {...baseProps()}
        running
        batchError="stale failure from the previous batch"
        counts={countsOf(['compressing'])}
      />
    )
    const pill = screen.getByTestId('status-pill')
    expect(pill.textContent).toBe('Compressing')
    expect(pill.getAttribute('data-state')).toBe('running')
    expect(screen.queryByTestId('batch-error')).not.toBeInTheDocument()
    expect(screen.getByTestId('cancel-all')).toBeInTheDocument()
    expect(screen.queryByTestId('new-batch')).not.toBeInTheDocument()
  })

  it('batch error shows Failed pill + error text and New batch available, no Cancel All', () => {
    render(
      <RunHeader
        {...baseProps()}
        batchError="worker not reachable"
        counts={countsOf(['failed'])}
      />
    )
    expect(screen.getByTestId('status-pill').textContent).toBe('Failed')
    expect(screen.getByTestId('batch-error').textContent).toBe('worker not reachable')
    expect(screen.queryByTestId('cancel-all')).not.toBeInTheDocument()
    expect(screen.queryByText('Cancel All')).not.toBeInTheDocument()
    expect(screen.getByTestId('new-batch')).toBeInTheDocument()
  })

  it('cancelled batch shows muted Cancelled pill, not Complete', () => {
    render(
      <RunHeader {...baseProps()} cancelled counts={countsOf(['cancelled', 'pending'])} />
    )
    const pill = screen.getByTestId('status-pill')
    expect(pill.textContent).toBe('Cancelled')
    expect(pill.getAttribute('data-state')).toBe('cancelled')
    expect(pill.className).toContain('text-muted-foreground')
    expect(pill.className).not.toContain('bg-primary')
    expect(screen.queryByTestId('compressing-dot')).not.toBeInTheDocument()
    expect(screen.getByTestId('new-batch')).toBeInTheDocument()
    expect(screen.queryByTestId('cancel-all')).not.toBeInTheDocument()
  })

  it('running still shows Compressing pill even with cancelled prop; error still shows Failed', () => {
    const { rerender } = render(
      <RunHeader {...baseProps()} running cancelled counts={countsOf(['compressing'])} />
    )
    const runningPill = screen.getByTestId('status-pill')
    expect(runningPill.textContent).toBe('Compressing')
    expect(runningPill.getAttribute('data-state')).toBe('running')

    rerender(
      <RunHeader
        {...baseProps()}
        cancelled
        batchError="worker gone"
        counts={countsOf(['failed'])}
      />
    )
    const failedPill = screen.getByTestId('status-pill')
    expect(failedPill.textContent).toBe('Failed')
    expect(failedPill.getAttribute('data-state')).toBe('failed')
  })

  it('Cancel All (destructive outline) while running, New batch (primary) otherwise, wired to handlers', () => {
    const { rerender } = render(
      <RunHeader {...baseProps()} running counts={countsOf(['compressing'])} />
    )
    const cancelAll = screen.getByTestId('cancel-all')
    expect(cancelAll.className).toContain('text-destructive')
    expect(cancelAll.className).toContain('border-destructive/30')
    expect(screen.queryByTestId('new-batch')).not.toBeInTheDocument()

    const cancelProps = { ...baseProps(), running: true, onCancelAll: vi.fn() }
    rerender(<RunHeader {...cancelProps} counts={countsOf(['compressing'])} />)
    fireEvent.click(screen.getByTestId('cancel-all'))
    expect(cancelProps.onCancelAll).toHaveBeenCalledTimes(1)

    const newProps = { ...baseProps(), onNewBatch: vi.fn() }
    rerender(<RunHeader {...newProps} counts={countsOf(['done'])} />)
    const newBatch = screen.getByTestId('new-batch')
    expect(newBatch.className).toContain('bg-primary')
    expect(screen.queryByTestId('cancel-all')).not.toBeInTheDocument()
    fireEvent.click(newBatch)
    expect(newProps.onNewBatch).toHaveBeenCalledTimes(1)
  })

  it('chips render roster counts, hide zero-count chips except All files, and expose aria-pressed', () => {
    render(
      <RunHeader
        {...baseProps()}
        counts={{
          all: 7,
          processing: 2,
          complete: 3,
          belowTarget: 1,
          failed: 0,
          queued: 1,
          waiting: 0,
          processed: 4,
        }}
      />
    )
    expect(screen.getByTestId('chip-all').textContent).toBe('All files7')
    expect(screen.getByTestId('chip-all').getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByTestId('chip-compressing').textContent).toBe('Processing2')
    expect(screen.getByTestId('chip-complete').textContent).toBe('Complete3')
    expect(screen.getByTestId('chip-below_target').textContent).toBe('Below target1')
    expect(screen.getByTestId('chip-queued').textContent).toBe('Queued1')
    expect(screen.queryByTestId('chip-failed')).not.toBeInTheDocument()
    expect(screen.getByTestId('chip-compressing').getAttribute('aria-pressed')).toBe('false')
  })

  it('clicking a chip calls onFilterChange with its value; clicking the active chip resets to all', () => {
    const first = { ...baseProps(), onFilterChange: vi.fn() }
    const { rerender } = render(
      <RunHeader
        {...first}
        counts={{
          all: 5,
          processing: 1,
          complete: 2,
          belowTarget: 0,
          failed: 1,
          queued: 1,
          waiting: 0,
          processed: 3,
        }}
      />
    )
    fireEvent.click(screen.getByTestId('chip-failed'))
    expect(first.onFilterChange).toHaveBeenCalledTimes(1)
    expect(first.onFilterChange).toHaveBeenCalledWith('failed')

    const second = { ...baseProps(), statusFilter: 'failed' as const, onFilterChange: vi.fn() }
    rerender(
      <RunHeader
        {...second}
        counts={{
          all: 5,
          processing: 1,
          complete: 2,
          belowTarget: 0,
          failed: 1,
          queued: 1,
          waiting: 0,
          processed: 3,
        }}
      />
    )
    expect(screen.getByTestId('chip-failed').getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByTestId('chip-failed').className).toContain('text-destructive')
    expect(screen.getByTestId('chip-failed').className).toContain('bg-destructive/10')
    expect(screen.getByTestId('chip-all').getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(screen.getByTestId('chip-failed'))
    expect(second.onFilterChange).toHaveBeenCalledTimes(1)
    expect(second.onFilterChange).toHaveBeenCalledWith('all')
  })

  it('All files chip click wires onFilterChange with "all"', () => {
    const props = { ...baseProps(), counts: countsOf(['done', 'failed']) }

    render(<RunHeader {...props} />)
    fireEvent.click(screen.getByTestId('chip-all'))
    expect(props.onFilterChange).toHaveBeenCalledTimes(1)
    expect(props.onFilterChange).toHaveBeenCalledWith('all')
  })

  it('processed label and progress bar width reflect processed/total', () => {
    const { rerender } = render(
      <RunHeader
        {...baseProps()}
        total={4}
        counts={countsOf(['done', 'best_effort', 'failed', 'pending'])}
      />
    )
    expect(screen.getByTestId('batch-progress').textContent).toContain('Batch: 3/4 processed')
    expect(screen.getByTestId('batch-progress-fill').style.width).toBe('75%')

    rerender(<RunHeader {...baseProps()} counts={countsOf([])} total={0} />)
    expect(screen.getByTestId('batch-progress').textContent).toContain('Batch: 0/0 processed')
    expect(screen.getByTestId('batch-progress-fill').style.width).toBe('0%')
  })

  it('chip counts come from the counts prop verbatim even when a narrower filter is active', () => {
    render(
      <RunHeader
        {...baseProps()}
        statusFilter="complete"
        counts={{
          all: 6,
          processing: 2,
          complete: 3,
          belowTarget: 0,
          failed: 1,
          queued: 0,
          waiting: 0,
          processed: 4,
        }}
      />
    )
    expect(screen.getByTestId('chip-all').textContent).toBe('All files6')
    expect(screen.getByTestId('chip-complete').textContent).toBe('Complete3')
    expect(screen.getByTestId('chip-complete').getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByTestId('chip-failed').textContent).toBe('Failed1')
  })
})
