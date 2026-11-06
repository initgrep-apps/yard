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

export type ViewId = 'compress' | 'history'

export const CATEGORIES = ['image', 'video', 'pdf', 'document', 'other'] as const

export type Category = (typeof CATEGORIES)[number]

export type FileStatus =
  | 'pending'
  | 'compressing'
  | 'done'
  | 'best_effort'
  | 'failed'
  | 'cancelled'
  | 'unsupported'
  | 'already_under_target'
  | 'needs_permission'

export type Change =
  | 'lossy_encode'
  | 'downscale'
  | 'drop_stream'
  | 'format_change'
  | 'metadata_strip'
  | 'rasterize'
  | 'page_drop'

export type ConsentSource = 'default' | 'global' | 'category' | 'batch' | 'once'

export interface FileEntry {
  id: string
  name: string
  path: string
  category: Category
  targetBytes: number
  originalSize: number
  status: FileStatus
  compressedSize: number | null
  outputPath: string | null
  error: string | null
  strategyUsed: string | null
  qualityUsed: number | null
}

export interface CategoryTarget {
  size: number
  unit: 'KB' | 'MB'
}

export interface QualityFloors {
  image: { minQuality: number; minLongEdge: number }
  video: { minCrf: number; minResolution: string }
  pdf: { minDpi: number; minJpegQuality: number }
  document: { minMediaQuality: number }
}

export interface PerformanceSettings {
  adaptiveMemory: boolean
  memoryCeilingMb: number
}

export interface HistorySettings {
  retentionDays: number
}

export interface ConsentSettings {
  allowDestructive: boolean
  heicToJpg: boolean
}

export interface Settings {
  outputFolder: string
  categoryTargets: Record<string, CategoryTarget>
  qualityFloors: QualityFloors
  performance: PerformanceSettings
  history: HistorySettings
  consent: ConsentSettings
}

export interface HistoryFile {
  name: string
  path: string
  outputPath: string | null
  category: Category
  targetBytes: number
  originalSize: number
  compressedSize: number | null
  status: FileStatus
  qualityUsed: number | null
  strategyUsed: string | null
  error: string | null
  durationMs: number
  completedAt: number
}

export interface SessionOutcome {
  done: number
  bestEffort: number
  failed: number
  cancelled: number
  unsupported: number
  alreadyUnderTarget: number
  needsPermission: number
}

export interface HistorySession {
  sessionId: string
  startedAt: number
  endedAt: number | null
  outputFolder: string
  fileCount: number
  files: HistoryFile[]
  outcome: SessionOutcome
  totalSavedBytes: number
  cancelled: boolean
  interrupted: boolean
  matchedFiles: number | null
}

export interface HistoryFilters {
  search?: string
  statuses?: FileStatus[]
  categories?: Category[]
  sinceMs?: number
  outputFolder?: string
  sort?: 'newest' | 'oldest'
}

export interface HistorySummary {
  sessions: number
  files: number
  savedBytes: number
  failedFiles: number
}

export interface HistoryResponse {
  sessions: HistorySession[]
  page: number
  pageSize: number
  totalSessions: number
  totalFiles: number
  summary: HistorySummary
  outputFolders: string[]
  skippedLines: number
}

