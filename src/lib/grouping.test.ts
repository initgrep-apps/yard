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

import { describe, it, expect, vi } from 'vitest'
import type { FileEntry, FileStatus } from '@/types'
import {
  LANE_ORDER,
  isTerminal,
  batchCounts,
  laneDonePill,
  filterFiles,
  type StatusFilter,
} from '@/lib/grouping'

let nextId = 0

const makeFile = (overrides: Partial<FileEntry> = {}): FileEntry => {
  const id = overrides.id ?? `f${++nextId}`
  return {
    id,
    name: `${id}.png`,
    path: `/tmp/${id}.png`,
    category: 'image',
    targetBytes: 1024,
    originalSize: 4096,
    status: 'pending',
    compressedSize: null,
    outputPath: null,
    error: null,
    strategyUsed: null,
    qualityUsed: null,
    ...overrides,
  }
}

describe('grouping', () => {
  it('LANE_ORDER is the fixed category order', () => {
    expect(LANE_ORDER).toEqual(['image', 'video', 'pdf', 'document', 'other'])
  })

  it('isTerminal truth table', () => {
    const terminal: FileStatus[] = ['done', 'best_effort', 'already_under_target', 'failed', 'cancelled']
    const active: FileStatus[] = ['pending', 'compressing', 'unsupported']
    for (const status of terminal) expect(isTerminal(status)).toBe(true)
    for (const status of active) expect(isTerminal(status)).toBe(false)
  })

  it('batchCounts excludes unsupported files from counts', () => {
    const files = [
      makeFile({ id: 'u1', category: 'pdf', status: 'unsupported' }),
      makeFile({ id: 'k', category: 'pdf', status: 'done' }),
      makeFile({ id: 'u2', category: 'other', status: 'unsupported' }),
      makeFile({ id: 'q', category: 'pdf', status: 'pending' }),
    ]
    expect(batchCounts(files)).toEqual({
      all: 2,
      processing: 0,
      complete: 1,
      belowTarget: 0,
      failed: 0,
      queued: 1,
      waiting: 0,
      processed: 1,
    })
  })

  it('batchCounts matches the spec 10-file scenario', () => {
    const files = [
      makeFile({ id: 'p1', status: 'compressing' }),
      makeFile({ id: 'p2', status: 'compressing' }),
      makeFile({ id: 'd1', status: 'done' }),
      makeFile({ id: 'd2', status: 'done' }),
      makeFile({ id: 'b1', status: 'best_effort' }),
      makeFile({ id: 'b2', status: 'best_effort' }),
      makeFile({ id: 't1', status: 'already_under_target' }),
      makeFile({ id: 'f1', status: 'failed' }),
      makeFile({ id: 'q1', status: 'pending' }),
      makeFile({ id: 'q2', status: 'pending' }),
    ]
    expect(batchCounts(files)).toEqual({
      all: 10,
      processing: 2,
      complete: 4,
      belowTarget: 1,
      failed: 1,
      queued: 2,
      waiting: 0,
      processed: 6,
    })
  })

  it('processed counts cancelled rows as terminal', () => {
    const files = [makeFile({ id: 'x', status: 'cancelled' }), makeFile({ id: 'y', status: 'done' })]
    const counts = batchCounts(files)
    expect(counts.processed).toBe(2)
    expect(counts.complete).toBe(1)
    expect(counts.failed).toBe(0)
    expect(laneDonePill(files)).toEqual({ done: 2, total: 2 })
  })

  it('laneDonePill returns done/total per lane', () => {
    expect(laneDonePill([])).toEqual({ done: 0, total: 0 })
    const lane = [
      makeFile({ id: 'l1', status: 'done' }),
      makeFile({ id: 'l2', status: 'compressing' }),
      makeFile({ id: 'l3', status: 'pending' }),
      makeFile({ id: 'l4', status: 'failed' }),
    ]
    expect(laneDonePill(lane)).toEqual({ done: 2, total: 4 })
  })

  it('filterFiles returns the right subset for every StatusFilter value', () => {
    const files = [
      makeFile({ id: 'c', status: 'compressing' }),
      makeFile({ id: 'd', status: 'done' }),
      makeFile({ id: 'b', status: 'best_effort' }),
      makeFile({ id: 't', status: 'already_under_target' }),
      makeFile({ id: 'f', status: 'failed' }),
      makeFile({ id: 'x', status: 'cancelled' }),
      makeFile({ id: 'q', status: 'pending' }),
      makeFile({ id: 'u', status: 'unsupported' }),
    ]
    const ids = (filter: StatusFilter) => filterFiles(files, filter).map((f) => f.id)
    expect(ids('all')).toEqual(['c', 'd', 'b', 't', 'f', 'x', 'q', 'u'])
    expect(ids('compressing')).toEqual(['c'])
    expect(ids('complete')).toEqual(['d', 'b'])
    expect(ids('below_target')).toEqual(['t'])
    expect(ids('failed')).toEqual(['f'])
    expect(ids('queued')).toEqual(['q'])
  })

  it('filtering does not mutate the input array', () => {
    const files = [
      makeFile({ id: 'c', status: 'compressing' }),
      makeFile({ id: 'd', status: 'done' }),
      makeFile({ id: 'q', status: 'pending' }),
    ]
    const snapshot = files.map((f) => ({ ...f }))
    filterFiles(files, 'complete')
    filterFiles(files, 'all')
    expect(files.map((f) => f.id)).toEqual(['c', 'd', 'q'])
    expect(files).toEqual(snapshot)
  })

  it('unknown runtime statuses are treated as processed-only with a warning', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const rawStatus: string = 'zzz_new'
    const file = makeFile({ id: 'u', status: rawStatus as FileStatus })
    try {
      expect(isTerminal(file.status)).toBe(true)
      expect(batchCounts([file])).toEqual({
        all: 1,
        processing: 0,
        complete: 0,
        belowTarget: 0,
        failed: 0,
        queued: 0,
        waiting: 0,
        processed: 1,
      })
      expect(laneDonePill([file])).toEqual({ done: 1, total: 1 })
      expect(warn).toHaveBeenCalledWith('unknown file status "zzz_new" treated as processed-only')
      expect(warn).toHaveBeenCalledTimes(1)
    } finally {
      warn.mockRestore()
    }
  })

  it('empty input yields zero counts', () => {
    expect(batchCounts([])).toEqual({
      all: 0,
      processing: 0,
      complete: 0,
      belowTarget: 0,
      failed: 0,
      queued: 0,
      waiting: 0,
      processed: 0,
    })
    expect(laneDonePill([])).toEqual({ done: 0, total: 0 })
    expect(filterFiles([], 'all')).toEqual([])
    expect(filterFiles([], 'complete')).toEqual([])
  })
})
