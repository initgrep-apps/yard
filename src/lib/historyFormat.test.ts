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

import { describe, it, expect } from 'vitest'
import {
  batchLabel,
  countOutcome,
  describeSince,
  fileCountLabel,
  formatDuration,
  presetSinceMs,
  dayLabel,
  dayKey,
  activeFilterCount,
  sessionSummary,
  DATE_PRESETS,
  DATE_PRESET_LABELS,
  savedLabel,
  savedPercent,
} from '@/lib/historyFormat'
import type { HistoryFile, HistorySession, SessionOutcome } from '@/types'

describe('historyFormat', () => {
  it('formats durations', () => {
    expect(formatDuration(0)).toBe('—')
    expect(formatDuration(45_000)).toBe('45s')
    expect(formatDuration(102_000)).toBe('1m 42s')
    expect(formatDuration(7_500_000)).toBe('2h 05m')
  })

  it('maps presets to local midnights', () => {
    const now = new Date(2026, 9, 2, 14, 30)
    const today = new Date(2026, 9, 2).getTime()
    expect(presetSinceMs('today', now)).toBe(today)
    expect(presetSinceMs('last7', now)).toBe(new Date(2026, 8, 26).getTime())
    expect(presetSinceMs('last30', now)).toBe(new Date(2026, 8, 3).getTime())
    expect(presetSinceMs('month', now)).toBe(new Date(2026, 9, 1).getTime())
    expect(presetSinceMs('last3months', now)).toBe(new Date(2026, 6, 2).getTime())
    expect(presetSinceMs('all', now)).toBeUndefined()
  })

  it('labels today and yesterday', () => {
    const now = new Date(2026, 9, 2, 14, 30)
    expect(dayLabel(new Date(2026, 9, 2, 9, 0).getTime(), now)).toBe('Today')
    expect(dayLabel(new Date(2026, 9, 1, 9, 0).getTime(), now)).toBe('Yesterday')
    const sep = dayLabel(new Date(2026, 8, 20).getTime(), now)
    expect(sep).toMatch(/Sep/)
    expect(sep).not.toMatch(/2026/)
    expect(dayLabel(new Date(2025, 8, 20).getTime(), now)).toMatch(/2025/)
  })

  it('pins DST transition midnights and year rollover', () => {
    const spring = new Date(2026, 2, 8, 15, 0)
    expect(presetSinceMs('today', spring)).toBe(new Date(2026, 2, 8).getTime())
    const fall = new Date(2026, 10, 1, 15, 0)
    expect(presetSinceMs('today', fall)).toBe(new Date(2026, 10, 1).getTime())
    const euSpring = new Date(2026, 2, 29, 15, 0)
    expect(presetSinceMs('today', euSpring)).toBe(new Date(2026, 2, 29).getTime())
    const euFall = new Date(2026, 9, 25, 15, 0)
    expect(presetSinceMs('today', euFall)).toBe(new Date(2026, 9, 25).getTime())
    const rollover = new Date(2027, 0, 1, 1, 0)
    expect(presetSinceMs('last7', rollover)).toBe(new Date(2026, 11, 26).getTime())
    const spanning = new Date(2026, 9, 26, 15, 0)
    expect(presetSinceMs('last7', spanning)).toBe(new Date(2026, 9, 20).getTime())
  })

  it('keys days by local calendar date', () => {
    const morning = new Date(2026, 9, 2, 9, 0).getTime()
    const night = new Date(2026, 9, 2, 23, 59).getTime()
    const nextDay = new Date(2026, 9, 3, 0, 1).getTime()
    expect(dayKey(morning)).toBe(dayKey(night))
    expect(dayKey(morning)).not.toBe(dayKey(nextDay))
  })

  it('formats duration boundaries', () => {
    expect(formatDuration(-5)).toBe('—')
    expect(formatDuration(Number.NaN)).toBe('—')
    expect(formatDuration(Number.POSITIVE_INFINITY)).toBe('—')
    expect(formatDuration(Number.NEGATIVE_INFINITY)).toBe('—')
    expect(formatDuration(999)).toBe('1s')
    expect(formatDuration(1_000)).toBe('1s')
    expect(formatDuration(59_999)).toBe('59s')
    expect(formatDuration(60_000)).toBe('1m 00s')
    expect(formatDuration(3_599_999)).toBe('59m 59s')
    expect(formatDuration(3_600_000)).toBe('1h 00m')
  })

  it('labels yesterday across year boundary', () => {
    const now = new Date(2027, 0, 1, 3, 0)
    expect(dayLabel(new Date(2026, 11, 31, 20, 0).getTime(), now)).toBe('Yesterday')
  })
})

const timeLabel = (ms: number): string =>
  new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' }).format(
    new Date(ms)
  )

describe('batchLabel', () => {
  const now = new Date(2026, 9, 23, 15, 0)

  it('labels today, yesterday and other days with the batch time', () => {
    const today = new Date(2026, 9, 23, 14, 31).getTime()
    const yesterday = new Date(2026, 9, 22, 18, 20).getTime()
    const other = new Date(2026, 9, 20, 16, 48).getTime()
    expect(batchLabel(today, now)).toBe(`Today · ${timeLabel(today)}`)
    expect(batchLabel(yesterday, now)).toBe(`Yesterday · ${timeLabel(yesterday)}`)
    expect(batchLabel(other, now)).toBe(`${dayLabel(other, now)} · ${timeLabel(other)}`)
  })
})

describe('describeSince', () => {
  const now = new Date(2026, 9, 23, 15, 0)

  it('names each preset', () => {
    expect(describeSince(undefined, now)).toBe('All time')
    expect(describeSince(presetSinceMs('today', now), now)).toBe('Today')
    expect(describeSince(presetSinceMs('last7', now), now)).toBe('Last 7 days')
    expect(describeSince(presetSinceMs('last30', now), now)).toBe('Last 30 days')
    expect(describeSince(presetSinceMs('month', now), now)).toBe('This month')
    expect(describeSince(presetSinceMs('last3months', now), now)).toBe(
      'Last 3 months',
    )
  })

  it.each(DATE_PRESETS.filter((preset) => preset !== 'all'))(
    'labels %s',
    (preset) => {
      expect(describeSince(presetSinceMs(preset, now), now)).toBe(
        DATE_PRESET_LABELS[preset],
      )
    },
  )

  it('falls back to the calendar date for a custom start', () => {
    const custom = new Date(2026, 8, 20).getTime()
    expect(describeSince(custom, now)).toBe(dayLabel(custom, now))
    expect(describeSince(new Date(2025, 8, 20).getTime(), now)).toMatch(/2025/)
  })

  it('guards non-finite timestamps', () => {
    expect(dayLabel(Number.NaN, now)).toBe('—')
    expect(dayLabel(Number.POSITIVE_INFINITY, now)).toBe('—')
    expect(batchLabel(Number.NaN, now)).toBe('—')
    expect(batchLabel(Number.POSITIVE_INFINITY, now)).toBe('—')
    expect(describeSince(Number.NaN, now)).toBe('—')
    expect(describeSince(Number.POSITIVE_INFINITY, now)).toBe('—')
  })
})

const fileWithStatus = (status: HistoryFile['status']): HistoryFile => ({
  name: `${status}.bin`,
  path: `/tmp/${status}.bin`,
  outputPath: null,
  category: 'other',
  targetBytes: 100,
  originalSize: 200,
  compressedSize: null,
  status,
  qualityUsed: null,
  strategyUsed: null,
  error: null,
  durationMs: 10,
  completedAt: 0,
  changes: [],
  consentSource: null,
})

describe('history aggregates', () => {
  it('formats saved bytes and file counts', () => {
    expect(savedLabel(0)).toBe('0 B')
    expect(savedLabel(1536)).toBe('2 KB')
    expect(savedLabel(Number.NaN)).toBe('—')
    expect(savedLabel(Number.POSITIVE_INFINITY)).toBe('—')
    expect(savedLabel(-1)).toBe('—')
    expect(fileCountLabel(0)).toBe('0 files')
    expect(fileCountLabel(1)).toBe('1 file')
    expect(fileCountLabel(12)).toBe('12 files')
    expect(fileCountLabel(Number.NaN)).toBe('—')
    expect(fileCountLabel(-1)).toBe('—')
  })

  it('computes the saved percentage or null when it is unknowable', () => {
    expect(savedPercent(100, 25)).toBe(75)
    expect(savedPercent(3, 2)).toBe(33)
    expect(savedPercent(100, 150)).toBe(0)
    expect(savedPercent(100, null)).toBeNull()
    expect(savedPercent(100, Number.NaN)).toBeNull()
    expect(savedPercent(Number.NaN, 10)).toBeNull()
    expect(savedPercent(0, 0)).toBeNull()
    expect(savedPercent(-10, 5)).toBeNull()
    expect(savedPercent(Number.POSITIVE_INFINITY, 10)).toBeNull()
    expect(savedPercent(100, Number.POSITIVE_INFINITY)).toBeNull()
    expect(savedPercent(100, -10)).toBeNull()
  })

  it('counts session outcomes', () => {
    const expected: SessionOutcome = {
      done: 2,
      bestEffort: 1,
      failed: 1,
      cancelled: 0,
      unsupported: 1,
      alreadyUnderTarget: 1,
      needsPermission: 0,
    }
    expect(
      countOutcome([
        fileWithStatus('done'),
        fileWithStatus('done'),
        fileWithStatus('best_effort'),
        fileWithStatus('failed'),
        fileWithStatus('unsupported'),
        fileWithStatus('already_under_target'),
        fileWithStatus('pending'),
        fileWithStatus('compressing'),
        fileWithStatus('bogus' as HistoryFile['status']),
        fileWithStatus('toString' as HistoryFile['status']),
      ])
    ).toEqual(expected)
  })
})

const sizedFile = (
  originalSize: number,
  compressedSize: number | null,
): HistoryFile => ({
  ...fileWithStatus(compressedSize === null ? 'failed' : 'done'),
  originalSize,
  compressedSize,
})

const sessionWith = (
  files: HistoryFile[],
  overrides: Partial<HistorySession> = {},
): HistorySession => ({
  sessionId: 's1',
  startedAt: 0,
  endedAt: 1,
  outputFolder: '/out',
  fileCount: files.length,
  files,
  outcome: {
    done: 0,
    bestEffort: 0,
    failed: 0,
    cancelled: 0,
    unsupported: 0,
    alreadyUnderTarget: 0,
    needsPermission: 0,
  },
  totalSavedBytes: 0,
  cancelled: false,
  interrupted: false,
  matchedFiles: null,
  ...overrides,
})

describe('sessionSummary', () => {
  it('treats an uncompressed file as no savings, not as a full saving', () => {
    const summary = sessionSummary(
      sessionWith([sizedFile(1_000, null), sizedFile(1_000, null)]),
    )

    expect(summary).toEqual({ fileCount: 2, savedBytes: 0, percent: 0 })
  })

  it('nets savings across done and failed files', () => {
    const summary = sessionSummary(
      sessionWith([sizedFile(1_000, 250), sizedFile(1_000, null)], {
        totalSavedBytes: 750,
      }),
    )

    expect(summary.fileCount).toBe(2)
    expect(summary.savedBytes).toBe(750)
    expect(summary.percent).toBe(38)
  })

  it('trusts totalSavedBytes over the file sum for an unfiltered session', () => {
    const summary = sessionSummary(
      sessionWith([sizedFile(1_000, 250)], { totalSavedBytes: 500 }),
    )

    expect(summary.savedBytes).toBe(500)
    expect(summary.percent).toBe(50)
  })

  it('clamps a grown output instead of netting a negative saving', () => {
    const summary = sessionSummary(
      sessionWith([sizedFile(1_000, 1_500)], { matchedFiles: 1 }),
    )

    expect(summary.savedBytes).toBe(0)
    expect(summary.percent).toBe(0)
  })

  it('returns a null percent when the population has no original bytes', () => {
    expect(sessionSummary(sessionWith([])).percent).toBeNull()
  })

  it('uses the matched count and matched file sum for a filtered session', () => {
    const summary = sessionSummary(
      sessionWith([sizedFile(1_000, 250)], {
        fileCount: 9,
        matchedFiles: 3,
        totalSavedBytes: 999_999,
      }),
    )

    expect(summary.fileCount).toBe(3)
    expect(summary.savedBytes).toBe(750)
    expect(summary.percent).toBe(75)
  })

  it('counts the files actually seen for an interrupted session', () => {
    const summary = sessionSummary(
      sessionWith([sizedFile(1_000, 250)], {
        fileCount: 12,
        interrupted: true,
      }),
    )

    expect(summary.fileCount).toBe(1)
  })
})

describe('activeFilterCount', () => {
  it('ignores the default and empty filters', () => {
    expect(activeFilterCount({})).toBe(0)
    expect(activeFilterCount({ statuses: [], categories: [] })).toBe(0)
    expect(activeFilterCount({ search: '' })).toBe(0)
  })

  it('does not count sort because it only orders results', () => {
    expect(activeFilterCount({ sort: 'newest' })).toBe(0)
    expect(activeFilterCount({ sort: 'oldest' })).toBe(0)
  })

  it('counts every narrowing filter dimension', () => {
    expect(activeFilterCount({ search: 'hero' })).toBe(1)
    expect(activeFilterCount({ outputFolder: '/out' })).toBe(1)
    expect(activeFilterCount({ sinceMs: 1 })).toBe(1)
    expect(activeFilterCount({ statuses: ['failed', 'done'] })).toBe(2)
    expect(activeFilterCount({ categories: ['image'] })).toBe(1)
    expect(
      activeFilterCount({
        statuses: ['failed', 'done'],
        categories: ['image'],
        sinceMs: 1,
        sort: 'oldest',
        search: 'hero',
        outputFolder: '/out',
      }),
    ).toBe(6)
  })
})
