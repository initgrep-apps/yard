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

import type { Category, FileStatus } from '@/types'

interface Tone {
  readonly tint: string
  readonly text: string
  readonly border: string
  readonly accent: string
}

const MUTED: Tone = {
  tint: 'bg-muted',
  text: 'text-muted-foreground',
  border: 'border-border',
  accent: 'bg-muted-foreground',
}

const PRIMARY: Tone = {
  tint: 'bg-primary/12',
  text: 'text-primary',
  border: 'border-primary/30',
  accent: 'bg-primary',
}

const AMBER: Tone = {
  tint: 'bg-amber-500/12',
  text: 'text-amber-600',
  border: 'border-amber-500/30',
  accent: 'bg-amber-500',
}

const DESTRUCTIVE: Tone = {
  tint: 'bg-destructive/15',
  text: 'text-destructive',
  border: 'border-destructive/35',
  accent: 'bg-destructive',
}

export const CATEGORY_TONES: Record<Category, Tone> = {
  image: {
    tint: 'bg-blue-500/12',
    text: 'text-blue-600',
    border: 'border-blue-500/30',
    accent: 'bg-blue-500',
  },
  video: {
    tint: 'bg-violet-500/12',
    text: 'text-violet-600',
    border: 'border-violet-500/30',
    accent: 'bg-violet-500',
  },
  pdf: DESTRUCTIVE,
  document: AMBER,
  other: MUTED,
}

export const STATUS_TONES: Record<FileStatus, Tone> = {
  pending: MUTED,
  compressing: PRIMARY,
  done: {
    tint: 'bg-primary/15',
    text: 'text-primary',
    border: 'border-primary/30',
    accent: 'bg-primary',
  },
  best_effort: AMBER,
  failed: DESTRUCTIVE,
  cancelled: MUTED,
  unsupported: MUTED,
  already_under_target: PRIMARY,
  needs_permission: AMBER,
}
