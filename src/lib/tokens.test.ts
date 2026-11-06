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

import { describe, expect, it } from 'vitest'
import { CATEGORY_TONES, STATUS_TONES } from '@/lib/tokens'
import type { Category, FileStatus } from '@/types'

const CATEGORY_KEYS: Record<Category, true> = {
  image: true, video: true, pdf: true, document: true, other: true,
}
const STATUS_KEYS: Record<FileStatus, true> = {
  pending: true, compressing: true, done: true, best_effort: true,
  failed: true, cancelled: true, unsupported: true, already_under_target: true,
  needs_permission: true,
}
const ALL_CATEGORIES = Object.keys(CATEGORY_KEYS) as Category[]
const ALL_STATUSES = Object.keys(STATUS_KEYS) as FileStatus[]

const TINT_BAND = '1[0-5]'
const BORDER_BAND = '2[5-9]|3[0-5]'

describe('CATEGORY_TONES', () => {
  it('covers every category', () => {
    for (const c of ALL_CATEGORIES) {
      expect(CATEGORY_TONES[c]).toBeDefined()
      expect(CATEGORY_TONES[c].tint).toMatch(/^bg-/)
      expect(CATEGORY_TONES[c].text).toMatch(/^text-/)
      expect(CATEGORY_TONES[c].border).toMatch(/^border-/)
      expect(CATEGORY_TONES[c].accent).toMatch(/^bg-/)
    }
  })
  it('key set matches every Category value', () => {
    expect(Object.keys(CATEGORY_TONES).sort()).toEqual(
      Object.keys(CATEGORY_KEYS).sort()
    )
  })
  it('maps image=blue, video=violet, pdf=destructive, document=amber, other=muted', () => {
    expect(CATEGORY_TONES.image.text).toBe('text-blue-600')
    expect(CATEGORY_TONES.video.text).toBe('text-violet-600')
    expect(CATEGORY_TONES.pdf.text).toBe('text-destructive')
    expect(CATEGORY_TONES.document.text).toBe('text-amber-600')
    expect(CATEGORY_TONES.other.text).toBe('text-muted-foreground')
  })
  it('image/document/other carry expected tint, border and accent classes', () => {
    expect(CATEGORY_TONES.image.tint).toBe('bg-blue-500/12')
    expect(CATEGORY_TONES.image.border).toBe('border-blue-500/30')
    expect(CATEGORY_TONES.image.accent).toBe('bg-blue-500')
    expect(CATEGORY_TONES.document.tint).toBe('bg-amber-500/12')
    expect(CATEGORY_TONES.document.border).toBe('border-amber-500/30')
    expect(CATEGORY_TONES.document.accent).toBe('bg-amber-500')
    expect(CATEGORY_TONES.other.tint).toBe('bg-muted')
    expect(CATEGORY_TONES.other.border).toBe('border-border')
    expect(CATEGORY_TONES.other.accent).toBe('bg-muted-foreground')
  })
  it('video/pdf carry expected accent classes', () => {
    expect(CATEGORY_TONES.video.accent).toBe('bg-violet-500')
    expect(CATEGORY_TONES.pdf.accent).toBe('bg-destructive')
  })
})

describe('STATUS_TONES', () => {
  it('covers every status', () => {
    for (const s of ALL_STATUSES) {
      expect(STATUS_TONES[s]).toBeDefined()
      expect(STATUS_TONES[s].tint).toMatch(/^bg-/)
      expect(STATUS_TONES[s].text).toMatch(/^text-/)
      expect(STATUS_TONES[s].border).toMatch(/^border-/)
      expect(STATUS_TONES[s].accent).toMatch(/^bg-/)
    }
  })
  it('key set matches every FileStatus value', () => {
    expect(Object.keys(STATUS_TONES).sort()).toEqual(
      Object.keys(STATUS_KEYS).sort()
    )
  })
  it('maps done/already_under_target/compressing=primary, best_effort=amber, failed=destructive, cancelled/pending/unsupported=muted', () => {
    expect(STATUS_TONES.done.text).toBe('text-primary')
    expect(STATUS_TONES.already_under_target.text).toBe('text-primary')
    expect(STATUS_TONES.compressing.text).toBe('text-primary')
    expect(STATUS_TONES.best_effort.text).toBe('text-amber-600')
    expect(STATUS_TONES.failed.text).toBe('text-destructive')
    expect(STATUS_TONES.cancelled.text).toBe('text-muted-foreground')
    expect(STATUS_TONES.pending.text).toBe('text-muted-foreground')
    expect(STATUS_TONES.unsupported.text).toBe('text-muted-foreground')
  })
  it('done/best_effort/failed carry expected tint, border and accent classes', () => {
    expect(STATUS_TONES.done.tint).toBe('bg-primary/15')
    expect(STATUS_TONES.done.border).toBe('border-primary/30')
    expect(STATUS_TONES.done.accent).toBe('bg-primary')
    expect(STATUS_TONES.best_effort.tint).toBe('bg-amber-500/12')
    expect(STATUS_TONES.best_effort.border).toBe('border-amber-500/30')
    expect(STATUS_TONES.best_effort.accent).toBe('bg-amber-500')
    expect(STATUS_TONES.failed.tint).toBe('bg-destructive/15')
    expect(STATUS_TONES.failed.border).toBe('border-destructive/35')
    expect(STATUS_TONES.failed.accent).toBe('bg-destructive')
  })
  it('compressing/already_under_target/pending carry expected accent classes', () => {
    expect(STATUS_TONES.compressing.accent).toBe('bg-primary')
    expect(STATUS_TONES.already_under_target.accent).toBe('bg-primary')
    expect(STATUS_TONES.pending.accent).toBe('bg-muted-foreground')
  })
  it('pending/cancelled/unsupported carry expected tint, border and accent classes', () => {
    expect(STATUS_TONES.pending.tint).toBe('bg-muted')
    expect(STATUS_TONES.pending.border).toBe('border-border')
    expect(STATUS_TONES.cancelled.tint).toBe('bg-muted')
    expect(STATUS_TONES.cancelled.border).toBe('border-border')
    expect(STATUS_TONES.cancelled.accent).toBe('bg-muted-foreground')
    expect(STATUS_TONES.unsupported.tint).toBe('bg-muted')
    expect(STATUS_TONES.unsupported.border).toBe('border-border')
    expect(STATUS_TONES.unsupported.accent).toBe('bg-muted-foreground')
  })
})

describe('translucency contract', () => {
  it('alpha-composed tints stay between /10 and /15', () => {
    expect(CATEGORY_TONES.image.tint).toMatch(new RegExp(`^bg-blue-500/${TINT_BAND}$`))
    expect(CATEGORY_TONES.video.tint).toMatch(new RegExp(`^bg-violet-500/${TINT_BAND}$`))
    expect(CATEGORY_TONES.document.tint).toMatch(new RegExp(`^bg-amber-500/${TINT_BAND}$`))
    expect(CATEGORY_TONES.pdf.tint).toMatch(new RegExp(`^bg-destructive/${TINT_BAND}$`))
    expect(STATUS_TONES.compressing.tint).toMatch(new RegExp(`^bg-primary/${TINT_BAND}$`))
    expect(STATUS_TONES.done.tint).toMatch(new RegExp(`^bg-primary/${TINT_BAND}$`))
    expect(STATUS_TONES.already_under_target.tint).toMatch(new RegExp(`^bg-primary/${TINT_BAND}$`))
    expect(STATUS_TONES.best_effort.tint).toMatch(new RegExp(`^bg-amber-500/${TINT_BAND}$`))
    expect(STATUS_TONES.failed.tint).toMatch(new RegExp(`^bg-destructive/${TINT_BAND}$`))
  })
  it('alpha-composed borders stay between /25 and /35', () => {
    expect(CATEGORY_TONES.image.border).toMatch(new RegExp(`^border-blue-500/(${BORDER_BAND})$`))
    expect(CATEGORY_TONES.video.border).toMatch(new RegExp(`^border-violet-500/(${BORDER_BAND})$`))
    expect(CATEGORY_TONES.document.border).toMatch(new RegExp(`^border-amber-500/(${BORDER_BAND})$`))
    expect(CATEGORY_TONES.pdf.border).toMatch(new RegExp(`^border-destructive/(${BORDER_BAND})$`))
    expect(STATUS_TONES.compressing.border).toMatch(new RegExp(`^border-primary/(${BORDER_BAND})$`))
    expect(STATUS_TONES.done.border).toMatch(new RegExp(`^border-primary/(${BORDER_BAND})$`))
    expect(STATUS_TONES.already_under_target.border).toMatch(new RegExp(`^border-primary/(${BORDER_BAND})$`))
    expect(STATUS_TONES.best_effort.border).toMatch(new RegExp(`^border-amber-500/(${BORDER_BAND})$`))
    expect(STATUS_TONES.failed.border).toMatch(new RegExp(`^border-destructive/(${BORDER_BAND})$`))
  })
})
