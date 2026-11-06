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

import { startOfDay, startOfMonth, subDays, subMonths } from 'date-fns'
import { formatSize } from '@/lib/sizeFormat'
import type {
  FileStatus,
  HistoryFile,
  HistoryFilters,
  HistorySession,
  SessionOutcome,
} from '@/types'

export function savedLabel(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—'
  return bytes === 0 ? '0 B' : formatSize(bytes)
}

export function fileCountLabel(count: number): string {
  if (!Number.isFinite(count) || count < 0) return '—'
  return count === 1 ? '1 file' : `${count} files`
}

export function savedPercent(original: number, compressed: number | null): number | null {
  if (!Number.isFinite(original) || original <= 0) return null
  if (compressed === null || !Number.isFinite(compressed) || compressed < 0) return null
  return Math.min(100, Math.max(0, Math.round(((original - compressed) / original) * 100)))
}

export function sessionSummary(session: HistorySession): {
  fileCount: number
  savedBytes: number
  percent: number | null
} {
  const matched = session.matchedFiles !== null
  const totalOriginal = session.files.reduce(
    (sum, file) => sum + file.originalSize,
    0,
  )
  const savedBytes = matched
    ? session.files.reduce(
        (sum, file) =>
          sum +
          Math.max(
            0,
            file.originalSize - (file.compressedSize ?? file.originalSize),
          ),
        0,
      )
    : session.totalSavedBytes
  return {
    fileCount:
      session.matchedFiles ??
      (session.interrupted ? session.files.length : session.fileCount),
    savedBytes,
    percent: savedPercent(totalOriginal, totalOriginal - savedBytes),
  }
}

export function activeFilterCount(filters: HistoryFilters): number {
  return (
    (filters.statuses?.length ?? 0) +
    (filters.categories?.length ?? 0) +
    (filters.sinceMs !== undefined ? 1 : 0) +
    (filters.search ? 1 : 0) +
    (filters.outputFolder ? 1 : 0)
  )
}

const OUTCOME_COUNT_KEYS: Record<FileStatus, keyof SessionOutcome | null> = {
  pending: null,
  compressing: null,
  done: 'done',
  best_effort: 'bestEffort',
  failed: 'failed',
  cancelled: 'cancelled',
  unsupported: 'unsupported',
  already_under_target: 'alreadyUnderTarget',
  needs_permission: 'needsPermission',
}

export function countOutcome(files: HistoryFile[]): SessionOutcome {
  const counts: SessionOutcome = {
    done: 0,
    bestEffort: 0,
    failed: 0,
    cancelled: 0,
    unsupported: 0,
    alreadyUnderTarget: 0,
    needsPermission: 0,
  }
  return files.reduce((acc, file) => {
    const key = Object.prototype.hasOwnProperty.call(OUTCOME_COUNT_KEYS, file.status)
      ? OUTCOME_COUNT_KEYS[file.status]
      : null
    if (key != null) acc[key] += 1
    return acc
  }, counts)
}

export function batchLabel(ms: number, now: Date): string {
  if (!Number.isFinite(ms)) return '—'
  const date = new Date(ms)
  if (!Number.isFinite(date.getTime())) return '—'
  const time = new Intl.DateTimeFormat(undefined, {
    hour: 'numeric',
    minute: '2-digit',
  }).format(date)
  return `${dayLabel(ms, now)} · ${time}`
}

export function describeSince(ms: number | undefined, now: Date): string {
  if (ms === undefined) return DATE_PRESET_LABELS.all
  if (!Number.isFinite(ms)) return '—'
  for (const preset of DATE_PRESETS) {
    if (preset === 'all') continue
    if (ms === presetSinceMs(preset, now)) return DATE_PRESET_LABELS[preset]
  }
  return dayLabel(ms, now)
}

export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '—'
  if (ms < 60_000) {
    const seconds = Math.max(1, Math.floor(ms / 1_000))
    return `${seconds}s`
  }
  if (ms < 3_600_000) {
    const minutes = Math.floor(ms / 60_000)
    const seconds = Math.floor((ms % 60_000) / 1_000)
    return `${minutes}m ${String(seconds).padStart(2, '0')}s`
  }
  const hours = Math.floor(ms / 3_600_000)
  const minutes = Math.floor((ms % 3_600_000) / 60_000)
  return `${hours}h ${String(minutes).padStart(2, '0')}m`
}

export const DATE_PRESETS = [
  'all',
  'today',
  'last7',
  'last30',
  'month',
  'last3months',
] as const

export type DatePreset = (typeof DATE_PRESETS)[number]

export const DATE_PRESET_LABELS: Record<DatePreset, string> = {
  all: 'All time',
  today: 'Today',
  last7: 'Last 7 days',
  last30: 'Last 30 days',
  month: 'This month',
  last3months: 'Last 3 months',
}

export function presetSinceMs(preset: DatePreset, now: Date): number | undefined {
  switch (preset) {
    case 'all':
      return undefined
    case 'today':
      return startOfDay(now).getTime()
    case 'last7':
      return startOfDay(subDays(now, 6)).getTime()
    case 'last30':
      return startOfDay(subDays(now, 29)).getTime()
    case 'month':
      return startOfMonth(now).getTime()
    case 'last3months':
      return startOfDay(subMonths(now, 3)).getTime()
  }
}

export function dayKey(ms: number): string {
  const d = new Date(ms)
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`
}

export function dayLabel(ms: number, now: Date): string {
  const date = new Date(ms)
  if (!Number.isFinite(ms) || !Number.isFinite(date.getTime())) return '—'
  const key = dayKey(ms)
  if (key === dayKey(now.getTime())) return 'Today'
  const yesterday = new Date(now)
  yesterday.setDate(yesterday.getDate() - 1)
  if (key === dayKey(yesterday.getTime())) return 'Yesterday'
  const includeYear = date.getFullYear() !== now.getFullYear()
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    ...(includeYear ? { year: 'numeric' } : {}),
  }).format(date)
}
