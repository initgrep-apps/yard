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

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, renderHook, waitFor } from '@testing-library/react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

import { useMemory, type BatchMemoryUsage } from '@/hooks/useMemory'

describe('useMemory', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset()
    vi.mocked(listen).mockReset()
  })

  it('loads memory stats on mount', async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      totalRamBytes: 16 * 1024 * 1024 * 1024,
      budgetBytes: 2 * 1024 * 1024 * 1024,
      processRssBytes: 128 * 1024 * 1024,
    })

    const { result } = renderHook(() => useMemory())
    await waitFor(() =>
      expect(result.current.stats).toEqual({
        totalRamBytes: 16 * 1024 * 1024 * 1024,
        budgetBytes: 2 * 1024 * 1024 * 1024,
        processRssBytes: 128 * 1024 * 1024,
      }),
    )
    expect(invoke).toHaveBeenCalledWith('memory_stats')
  })

  it('updates batch usage from memory:usage events and clears it when the batch ends', async () => {
    const handlers: Record<string, (event: { payload: BatchMemoryUsage }) => void> = {}
    vi.mocked(listen).mockImplementation((async (
      event: string,
      cb: (event: { payload: BatchMemoryUsage }) => void,
    ) => {
      handlers[event] = cb
      return () => {}
    }) as typeof listen)
    vi.mocked(invoke).mockResolvedValue({
      totalRamBytes: 16 * 1024 * 1024 * 1024,
      budgetBytes: 2 * 1024 * 1024 * 1024,
      processRssBytes: 128 * 1024 * 1024,
    })

    const { result } = renderHook(() => useMemory())
    await waitFor(() => expect(handlers['memory:usage']).toBeDefined())

    const usage = { budgetBytes: 1024, usedBytes: 600, processRssBytes: 300 }
    act(() => {
      handlers['memory:usage']({ payload: usage })
    })
    expect(result.current.batchUsage).toEqual(usage)

    act(() => {
      handlers['batch:complete']({ payload: usage })
    })
    expect(result.current.batchUsage).toBeNull()

    act(() => {
      handlers['memory:usage']({ payload: usage })
    })
    expect(result.current.batchUsage).toEqual(usage)

    act(() => {
      handlers['batch:error']({ payload: usage })
    })
    expect(result.current.batchUsage).toBeNull()
  })
})
