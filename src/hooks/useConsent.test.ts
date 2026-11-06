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

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook, waitFor } from '@testing-library/react'

type EventHandler = (e: { payload: unknown }) => void

const registry = new Map<string, Set<EventHandler>>()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: EventHandler) => {
    if (!registry.has(event)) registry.set(event, new Set())
    registry.get(event)!.add(handler)
    return Promise.resolve(() => {
      registry.get(event)?.delete(handler)
    })
  }),
}))

import { __resetConsentStore, planPreflight, useConsent } from '@/hooks/useConsent'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { ConsentSettings, FileEntry } from '@/types'

function entry(name: string, index: number, overrides: Partial<FileEntry> = {}): FileEntry {
  return {
    id: `f${index}`,
    name,
    path: `/tmp/${name}`,
    category: 'image',
    targetBytes: 204_800,
    originalSize: 4_000_000,
    status: 'pending',
    compressedSize: null,
    outputPath: null,
    error: null,
    strategyUsed: null,
    qualityUsed: null,
    ...overrides,
  }
}

const heicFiles = (count: number): FileEntry[] =>
  Array.from({ length: count }, (_, index) => entry(`photo-${index}.heic`, index))

const consentOff: ConsentSettings = { allowDestructive: false, heicToJpg: false }
const consentGlobal: ConsentSettings = { allowDestructive: true, heicToJpg: false }
const consentHeicDefault: ConsentSettings = { allowDestructive: false, heicToJpg: true }

const emit = (payload: unknown) =>
  act(() => {
    registry.get('consent:required')?.forEach((handler) => handler({ payload }))
  })

beforeEach(() => {
  vi.clearAllMocks()
  __resetConsentStore()
})

describe('planPreflight', () => {
  it('fifty heic files produce one preflight prompt', () => {
    const plan = planPreflight(heicFiles(50), consentOff)

    expect(plan).not.toBeNull()
    expect(plan!.fileCount).toBe(50)
    expect(plan!.grants).toHaveLength(1)
    expect(plan!.grants[0]).toEqual({ category: 'image', changes: ['format_change'] })
  })

  it('a batch with no destructive change never prompts', () => {
    expect(planPreflight([entry('a.jpg', 0), entry('b.png', 1)], consentOff)).toBeNull()
    expect(planPreflight(heicFiles(10), consentGlobal)).toBeNull()
    expect(planPreflight(heicFiles(10), consentHeicDefault)).toBeNull()
  })

  it('already-under-target heic files never prompt, even fifty of them', () => {
    const alreadySmall = Array.from({ length: 50 }, (_, index) =>
      entry(`small-${index}.heic`, index, {
        originalSize: 100_000,
        targetBytes: 204_800,
      })
    )

    expect(planPreflight(alreadySmall, consentOff)).toBeNull()
  })

  it('already-under-target heic files never prompt, even fifty of them', () => {
    const alreadySmall = Array.from({ length: 50 }, (_, index) =>
      entry(`small-${index}.heic`, index, {
        originalSize: 100_000,
        targetBytes: 204_800,
      })
    )

    expect(planPreflight(alreadySmall, consentOff)).toBeNull()
  })
})

describe('useConsent requests', () => {
  it('tracks a live request and answers it once', async () => {
    const { result } = renderHook(() => useConsent())

    emit({ requestId: 'r1', fileId: 'f1', category: 'image', changes: ['downscale'], detail: 'Needs permission' })
    emit({ requestId: 'r2', fileId: 'f2', category: 'image', changes: ['downscale'], detail: 'Needs permission' })

    await waitFor(() => expect(result.current.waitingCount).toBe(2))
    expect(result.current.requestFor('f1')?.requestId).toBe('r1')

    await act(async () => {
      await result.current.answerConsent('r1', true, 'once')
    })

    expect(invoke).toHaveBeenCalledWith('answer_consent', {
      requestId: 'r1',
      allow: true,
      scope: 'once',
    })
    expect(result.current.requests.map((request) => request.requestId)).toEqual(['r2'])
  })

  it('apply to rest resolves requests already waiting', async () => {
    const { result } = renderHook(() => useConsent())

    emit({ requestId: 'r1', fileId: 'f1', category: 'image', changes: ['downscale'], detail: 'Needs permission' })
    emit({ requestId: 'r2', fileId: 'f2', category: 'image', changes: ['downscale'], detail: 'Needs permission' })
    emit({ requestId: 'r3', fileId: 'f3', category: 'image', changes: ['format_change'], detail: 'Needs permission' })
    emit({ requestId: 'r4', fileId: 'f4', category: 'video', changes: ['downscale'], detail: 'Needs permission' })

    await waitFor(() => expect(result.current.waitingCount).toBe(4))

    await act(async () => {
      await result.current.answerConsent('r1', true, 'batch')
    })

    expect(result.current.requests.map((request) => request.requestId)).toEqual(['r3', 'r4'])
    expect(invoke).toHaveBeenCalledWith('answer_consent', {
      requestId: 'r1',
      allow: true,
      scope: 'batch',
    })
  })

  it('retries listener registration after a failed mount', async () => {
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    vi.mocked(listen).mockRejectedValueOnce(new Error('no webview'))

    const first = renderHook(() => useConsent())
    await waitFor(() => expect(listen).toHaveBeenCalledTimes(1))
    await act(async () => {})
    first.unmount()

    renderHook(() => useConsent())
    await waitFor(() => expect(listen).toHaveBeenCalledTimes(2))

    expect(consoleSpy).toHaveBeenCalled()
    consoleSpy.mockRestore()
  })
})
