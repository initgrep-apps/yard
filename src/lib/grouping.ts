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

import type { FileEntry, FileStatus } from '@/types'
import { CATEGORIES } from '@/types'

export type StatusFilter = 'all' | 'compressing' | 'complete' | 'below_target' | 'failed' | 'queued'

export const LANE_ORDER = CATEGORIES

export interface BatchCounts {
  all: number
  processing: number
  complete: number
  belowTarget: number
  failed: number
  queued: number
  processed: number
}

type CountBucket = 'processing' | 'complete' | 'belowTarget' | 'failed' | 'queued'
type StatusBucket = CountBucket | 'processedOnly' | 'unsupported'

const STATUS_BUCKETS: Record<FileStatus, StatusBucket> = {
  pending: 'queued',
  compressing: 'processing',
  done: 'complete',
  best_effort: 'complete',
  failed: 'failed',
  cancelled: 'processedOnly',
  unsupported: 'unsupported',
  already_under_target: 'belowTarget',
  needs_permission: 'processedOnly',
}

const WARNED = new Set<string>()

function warnOnce(message: string): void {
  if (WARNED.has(message)) return
  WARNED.add(message)
  console.warn(message)
}

function bucketOf(status: FileStatus): StatusBucket {
  const bucket = STATUS_BUCKETS[status]
  if (bucket) return bucket
  warnOnce(`unknown file status "${status}" treated as processed-only`)
  return 'processedOnly'
}

function countsTowardProcessed(bucket: StatusBucket): boolean {
  return bucket !== 'unsupported' && bucket !== 'processing' && bucket !== 'queued'
}

export function isTerminal(status: FileStatus): boolean {
  return countsTowardProcessed(bucketOf(status))
}

export function batchCounts(files: FileEntry[]): BatchCounts {
  const counts: BatchCounts = {
    all: 0,
    processing: 0,
    complete: 0,
    belowTarget: 0,
    failed: 0,
    queued: 0,
    processed: 0,
  }
  for (const file of files) {
    const bucket = bucketOf(file.status)
    if (bucket === 'unsupported') continue
    counts.all += 1
    if (countsTowardProcessed(bucket)) counts.processed += 1
    if (bucket !== 'processedOnly') counts[bucket] += 1
  }
  return counts
}

export function laneDonePill(files: FileEntry[]): { done: number; total: number } {
  const inLane = files.filter((file) => bucketOf(file.status) !== 'unsupported')
  const done = inLane.filter((file) => countsTowardProcessed(bucketOf(file.status))).length
  return { done, total: inLane.length }
}

const FILTER_PREDICATES: Record<Exclude<StatusFilter, 'all'>, (status: FileStatus) => boolean> = {
  compressing: (status) => bucketOf(status) === 'processing',
  complete: (status) => bucketOf(status) === 'complete',
  below_target: (status) => bucketOf(status) === 'belowTarget',
  failed: (status) => bucketOf(status) === 'failed',
  queued: (status) => bucketOf(status) === 'queued',
}

export function filterFiles(files: FileEntry[], filter: StatusFilter): FileEntry[] {
  if (filter === 'all') return files.slice()
  const predicate = FILTER_PREDICATES[filter]
  return files.filter((file) => predicate(file.status))
}
