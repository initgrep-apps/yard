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

import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RunHeader } from '@/components/run/RunHeader'
import { RunBoard } from '@/components/run/RunBoard'
import { batchCounts, type StatusFilter } from '@/lib/grouping'
import type { FileEntry } from '@/types'

function makeFile(patch: Partial<FileEntry> = {}): FileEntry {
  return {
    id: 'f1',
    name: 'IMG_5647.MOV',
    path: '/tmp/IMG_5647.MOV',
    category: 'video',
    targetBytes: 8_388_608,
    originalSize: 1_100_000,
    status: 'pending',
    compressedSize: null,
    outputPath: null,
    error: null,
    strategyUsed: null,
    qualityUsed: null,
    ...patch,
  }
}

const LABEL_TO_CATEGORY: Record<string, string> = {
  Image: 'image',
  Video: 'video',
  PDF: 'pdf',
  Document: 'document',
  Other: 'other',
}

function laneLabels(): string[] {
  return Array.from(document.querySelectorAll('span')).flatMap((el) => {
    const category = LABEL_TO_CATEGORY[el.textContent ?? '']
    return category ? [category] : []
  })
}

const ALL_HANDLERS = {
  onFilterChange: vi.fn(),
  onCancel: vi.fn(),
  onReveal: vi.fn(),
  onRemove: vi.fn(),
}

function boardFor(roster: FileEntry[], statusFilter: StatusFilter = 'all') {
  return <RunBoard roster={roster} statusFilter={statusFilter} {...ALL_HANDLERS} />
}

describe('RunBoard', () => {
  it('renders one lane per category present in LANE_ORDER; single-category roster renders a single lane', () => {
    const mixed = [
      makeFile({ id: 'a', name: 'Photo.png', category: 'image' }),
      makeFile({ id: 'b', name: 'Clip.mov', category: 'video', status: 'done' }),
      makeFile({ id: 'c', name: 'Doc.pdf', category: 'pdf', status: 'done' }),
      makeFile({ id: 'd', name: 'Note.docx', category: 'document', status: 'done' }),
      makeFile({ id: 'e', name: 'Data.bin', category: 'other', status: 'done' }),
    ]
    const { rerender } = render(boardFor(mixed))
    expect(laneLabels()).toEqual(['image', 'video', 'pdf', 'document', 'other'])
    expect(screen.getByTestId('kanban-board').className).toContain('overflow-x-auto')
    const grid = document.querySelector('[data-slot="kanban-board"]') as HTMLElement
    expect(grid.className).toContain('w-max')
    expect(grid.className).toContain('min-w-full')
    expect(grid.style.gridTemplateColumns).toBe('repeat(5, 16rem)')

    rerender(boardFor([makeFile({ name: 'Photo.png', category: 'image' })]))
    expect(laneLabels()).toEqual(['image'])
    expect((document.querySelector('[data-slot="kanban-board"]') as HTMLElement).style.gridTemplateColumns).toBe('repeat(1, 16rem)')
  })

  it('failed filter renders only lanes containing failed rows', () => {
    render(
      boardFor(
        [
          makeFile({ id: 'a', name: 'Photo_done.png', category: 'image', status: 'done' }),
          makeFile({ id: 'b', name: 'Encode_Fail.mov', category: 'video', status: 'failed' }),
        ],
        'failed'
      )
    )
    expect(laneLabels()).toEqual(['video'])
    expect(screen.getByText('Encode_Fail.mov')).toBeInTheDocument()
    expect(screen.queryByText('Photo_done.png')).not.toBeInTheDocument()
  })

  it('filtered row unmounts when its status leaves the filter without crashing and header counts stay roster-based', async () => {
    const roster = [
      makeFile({ id: 'a', name: 'Drone_Loop.mp4', status: 'done' }),
      makeFile({ id: 'b', name: 'B-Roll.mp4', status: 'done' }),
    ]
    const onFilterChange = vi.fn()
    const headerProps = (files: FileEntry[]) => ({
      batchNumber: 1,
      running: true,
      batchError: null,
      counts: batchCounts(files),
      total: files.length,
      statusFilter: 'complete' as StatusFilter,
      onFilterChange,
      onCancelAll: vi.fn(),
      onNewBatch: vi.fn(),
    })
    const boardProps = (files: FileEntry[]) => ({
      roster: files,
      statusFilter: 'complete' as StatusFilter,
      onFilterChange,
      onCancel: vi.fn(),
      onReveal: vi.fn(),
      onRemove: vi.fn(),
    })
    const nextRoster = [
      makeFile({ id: 'a', name: 'Drone_Loop.mp4', status: 'compressing' }),
      makeFile({ id: 'b', name: 'B-Roll.mp4', status: 'done' }),
    ]
    const { rerender } = render(
      <div>
        <RunHeader {...headerProps(roster)} />
        <RunBoard {...boardProps(roster)} />
      </div>
    )
    expect(screen.getByTestId('chip-all').textContent).toBe('All files2')
    expect(screen.getByTestId('chip-complete').textContent).toBe('Complete2')

    rerender(
      <div>
        <RunHeader {...headerProps(nextRoster)} />
        <RunBoard {...boardProps(nextRoster)} />
      </div>
    )
    await waitFor(
      () => expect(screen.queryByText('Drone_Loop.mp4')).not.toBeInTheDocument(),
      { timeout: 2000 }
    )
    expect(screen.getByText('B-Roll.mp4')).toBeInTheDocument()
    expect(screen.getByTestId('chip-all').textContent).toBe('All files2')
    expect(screen.getByTestId('chip-complete').textContent).toBe('Complete1')
    expect(screen.getByTestId('chip-compressing').textContent).toBe('Processing1')
  })

  it('filtered-empty board shows Nothing {x} right now keyed to the active filter with a working Show all link', () => {
    const cases: Array<[StatusFilter, string, FileEntry[]]> = [
      ['compressing', 'Nothing processing right now', [makeFile({ status: 'done' })]],
      ['complete', 'Nothing complete right now', [makeFile({ status: 'compressing' })]],
      ['below_target', 'Nothing below target right now', [makeFile({ status: 'done' })]],
      ['failed', 'Nothing failed right now', [makeFile({ status: 'done' })]],
      ['queued', 'Nothing queued right now', [makeFile({ status: 'compressing' })]],
    ]
    for (const [filter, message, files] of cases) {
      const { unmount } = render(boardFor(files, filter))
      expect(screen.getByText(message)).toBeInTheDocument()
      expect(screen.getByTestId('show-all')).toBeInTheDocument()
      unmount()
    }

    const { unmount } = render(boardFor([], 'all'))
    expect(screen.getByText('Nothing here right now')).toBeInTheDocument()
    expect(screen.getByTestId('show-all')).toBeInTheDocument()
    unmount()
  })

  it('populated board rerenders to filtered-empty when every row leaves the filter, showing same-tree empty state', () => {
    const initial = [
      makeFile({ id: 'a', name: 'Fail_A.mov', status: 'failed' }),
      makeFile({ id: 'b', name: 'Fail_B.mov', status: 'failed' }),
    ]
    const next = [
      makeFile({ id: 'a', name: 'Fail_A.mov', status: 'done' }),
      makeFile({ id: 'b', name: 'Fail_B.mov', status: 'done' }),
    ]
    const propsFor = (roster: FileEntry[]) => ({
      roster,
      statusFilter: 'failed' as StatusFilter,
      onFilterChange: vi.fn(),
      onCancel: vi.fn(),
      onReveal: vi.fn(),
      onRemove: vi.fn(),
    })

    const { rerender } = render(<RunBoard {...propsFor(initial)} />)
    expect(laneLabels()).toEqual(['video'])

    rerender(<RunBoard {...propsFor(next)} />)
    expect(screen.queryByTestId('kanban-board')).not.toBeInTheDocument()
    expect(screen.getByText('Nothing failed right now')).toBeInTheDocument()
    expect(screen.getByTestId('show-all')).toBeInTheDocument()
    expect(screen.queryByText('Fail_A.mov')).not.toBeInTheDocument()
    expect(screen.queryByText('Fail_B.mov')).not.toBeInTheDocument()
  })

  it('Show all link resets the filter to all', () => {
    const onFilterChange = vi.fn()
    render(
      <RunBoard
        roster={[makeFile({ status: 'done' })]}
        statusFilter="failed"
        onFilterChange={onFilterChange}
        onCancel={vi.fn()}
        onReveal={vi.fn()}
        onRemove={vi.fn()}
      />
    )
    fireEvent.click(screen.getByTestId('show-all'))
    expect(onFilterChange).toHaveBeenCalledTimes(1)
    expect(onFilterChange).toHaveBeenCalledWith('all')
  })

  it('fills its container height and keeps the lane scroll chain intact', () => {
    render(boardFor([makeFile()]))
    const board = document.querySelector('[data-testid="kanban-board"]') as HTMLElement
    expect(board.className).toContain('h-full')
    expect(board.className).toContain('pr-6')
    expect(document.body.innerHTML).not.toContain('max-h-[533px]')
    expect(document.querySelectorAll('[class*="to-card"]')).toHaveLength(0)
    const kanbanRoot = document.querySelector('[data-slot="kanban"]') as HTMLElement
    expect(kanbanRoot.className).toContain('h-full')
    expect(kanbanRoot.className).toContain('min-h-0')
    const column = document.querySelector('[data-slot="kanban-column"]') as HTMLElement
    expect(column.className).toContain('h-full')
    expect(column.className).toContain('min-h-0')
    const inner = column.firstElementChild as HTMLElement
    expect(inner.className).toContain('h-full')
    expect(inner.className).toContain('flex-col')
    const content = document.querySelector('[data-slot="kanban-column-content"]') as HTMLElement
    expect(content.className).toContain('mt-2')
    expect(content.className).toContain('pl-3')
    expect(content.className).toContain('pr-3.5')
    expect(content.className).toContain('pb-3')
    expect(content.className).toContain('min-h-0')
    expect(content.className).toContain('flex-1')
    expect(content.className).toContain('overflow-y-auto')
    expect(content.hasAttribute('data-lane-scroll')).toBe(true)
  })

  it('removeDisabled suppresses card remove buttons; otherwise Remove is wired', () => {
    const onRemove = vi.fn()
    const props = {
      roster: [makeFile({ id: 'f1', name: 'Remove.png' })],
      statusFilter: 'all' as const,
      onFilterChange: vi.fn(),
      onCancel: vi.fn(),
      onReveal: vi.fn(),
      onRemove,
    }
    const { rerender } = render(<RunBoard {...props} removeDisabled />)
    expect(screen.queryByRole('button', { name: 'Remove Remove.png' })).not.toBeInTheDocument()

    rerender(<RunBoard {...props} />)
    fireEvent.click(screen.getByRole('button', { name: 'Remove Remove.png' }))
    expect(onRemove).toHaveBeenCalledTimes(1)
    expect(onRemove).toHaveBeenCalledWith('f1')
  })

  it('lane pill counts the unfiltered roster and live-tints while any lane file is compressing', () => {
    const onRemove = vi.fn()
    const props = (files: FileEntry[]) => ({
      roster: files,
      statusFilter: 'complete' as StatusFilter,
      onFilterChange: vi.fn(),
      onCancel: vi.fn(),
      onReveal: vi.fn(),
      onRemove,
    })
    const { rerender } = render(
      <RunBoard
        {...props([
          makeFile({ id: 'a', name: 'Drone_Loop.mp4', status: 'done', compressedSize: 700_000 }),
          makeFile({ id: 'b', name: 'Queued_Clip.mp4', status: 'pending' }),
        ])}
      />
    )
    expect(screen.queryByText('Queued_Clip.mp4')).not.toBeInTheDocument()
    const pill = screen.getByTestId('lane-pill')
    expect(pill.textContent).toBe('1/2 done')
    expect(pill.className).toContain('bg-muted')
    expect(pill.className).not.toContain('bg-primary/10')

    rerender(
      <RunBoard
        {...props([
          makeFile({ id: 'a', name: 'Drone_Loop.mp4', status: 'compressing' }),
          makeFile({ id: 'b', name: 'Queued_Clip.mp4', status: 'done', compressedSize: 700_000 }),
        ])}
      />
    )
    const livePill = screen.getByTestId('lane-pill')
    expect(livePill.textContent).toBe('1/2 done')
    expect(livePill.className).toContain('bg-primary/10')
    expect(livePill.className).toContain('text-primary')
    expect(livePill.className).not.toContain('bg-muted')
  })

  it('cards render progress percent while compressing and a result chip when done', () => {
    const { unmount } = render(
      <RunBoard
        roster={[makeFile({ status: 'compressing' })]}
        statusFilter="all"
        progress={{ f1: 45 }}
        {...ALL_HANDLERS}
      />
    )
    expect(screen.getByTestId('run-card-percent').textContent).toBe('45%')
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '45')
    unmount()

    render(boardFor([makeFile({ status: 'done', compressedSize: 700_000 })]))
    expect(screen.getByTestId('run-chip').textContent).toBe('-36%')
  })

  it('unsupported files never render a card', () => {
    render(
      boardFor([
        makeFile({ id: 'a', name: 'Okay.mov', status: 'done', compressedSize: 700_000 }),
        makeFile({ id: 'b', name: 'Bad.bin', status: 'unsupported' }),
      ])
    )
    expect(screen.getByText('Okay.mov')).toBeInTheDocument()
    expect(screen.queryByText('Bad.bin')).not.toBeInTheDocument()
    expect(document.querySelectorAll('[data-slot="kanban-item"]')).toHaveLength(1)
  })

  it('pending card jumps to lane front when its status becomes compressing mid-batch', () => {
    const props = (files: FileEntry[]) => ({
      roster: files,
      statusFilter: 'all' as StatusFilter,
      onFilterChange: vi.fn(),
      onCancel: vi.fn(),
      onReveal: vi.fn(),
      onRemove: vi.fn(),
    })
    const laneOrder = () => {
      const lane = document.querySelector('[data-lane-scroll]') as HTMLElement
      const text = lane.textContent ?? ''
      return (name: string) => text.indexOf(name)
    }
    const { rerender } = render(
      <RunBoard
        {...props([
          makeFile({ id: 'a', name: 'Alpha_Done.mov', status: 'done', compressedSize: 700_000 }),
          makeFile({ id: 'b', name: 'Beta_Queue.mov', status: 'pending' }),
        ])}
      />
    )
    expect(laneOrder()('Alpha_Done.mov')).toBeLessThan(laneOrder()('Beta_Queue.mov'))

    rerender(
      <RunBoard
        {...props([
          makeFile({ id: 'a', name: 'Alpha_Done.mov', status: 'done', compressedSize: 700_000 }),
          makeFile({ id: 'b', name: 'Beta_Queue.mov', status: 'compressing' }),
        ])}
      />
    )
    expect(laneOrder()('Beta_Queue.mov')).toBeLessThan(laneOrder()('Alpha_Done.mov'))

    rerender(
      <RunBoard
        {...props([
          makeFile({ id: 'a', name: 'Alpha_Done.mov', status: 'done', compressedSize: 700_000 }),
          makeFile({ id: 'b', name: 'Beta_Done.mov', status: 'done', compressedSize: 700_000 }),
        ])}
      />
    )
    expect(screen.getByText('Beta_Done.mov')).toBeInTheDocument()
    expect(laneOrder()('Beta_Done.mov')).toBeLessThan(laneOrder()('Alpha_Done.mov'))
  })

  it('lane pill excludes unsupported files from its counts', () => {
    render(
      boardFor([
        makeFile({ id: 'a', name: 'Okay.mov', status: 'done', compressedSize: 700_000 }),
        makeFile({ id: 'b', name: 'Bad.bin', status: 'unsupported' }),
      ])
    )
    expect(screen.getByTestId('lane-pill').textContent).toBe('1/1 done')
  })

  it('lane grip handle is always visible', () => {
    render(boardFor([makeFile({ status: 'done', compressedSize: 700_000 })]))
    const handle = document.querySelector('[data-slot="kanban-column-handle"]') as HTMLElement
    expect(handle.className).toContain('opacity-100')
  })
})
