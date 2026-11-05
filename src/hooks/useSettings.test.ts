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
import { renderHook, act } from '@testing-library/react'
import { invoke } from '@tauri-apps/api/core'
import { toast } from 'sonner'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))
vi.mock('sonner', () => ({ toast: { error: vi.fn(), success: vi.fn() } }))

import { useSettings } from '@/hooks/useSettings'
import type { Settings } from '@/types'

const mockSettings: Settings = {
  outputFolder: '/tmp/out',
  categoryTargets: {
    image: { size: 500, unit: 'KB' },
    video: { size: 10, unit: 'MB' },
    pdf: { size: 2, unit: 'MB' },
    document: { size: 1, unit: 'MB' },
    other: { size: 5, unit: 'MB' },
  },
  qualityFloors: {
    image: { minQuality: 60, minLongEdge: 1280 },
    video: { minCrf: 28, minResolution: '1280x720' },
    pdf: { minDpi: 100, minJpegQuality: 55 },
    document: { minMediaQuality: 50 },
  },
  performance: { adaptiveMemory: true, memoryCeilingMb: 2048 },
  history: { retentionDays: 90 },
}

beforeEach(() => vi.clearAllMocks())

describe('useSettings', () => {
  it('loads settings on mount', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(mockSettings)
    const { result } = renderHook(() => useSettings())
    expect(result.current.loading).toBe(true)
    await act(async () => {})
    expect(invoke).toHaveBeenCalledWith('get_settings')
    expect(result.current.settings).toEqual(mockSettings)
    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBeNull()
  })

  it('saveSettings invokes save_settings and syncs state', async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce(mockSettings)
      .mockResolvedValueOnce(undefined)
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    const updated: Settings = { ...mockSettings, outputFolder: '/tmp/new' }
    let saved: boolean | undefined
    await act(async () => {
      saved = await result.current.saveSettings(updated)
    })
    expect(saved).toBe(true)
    expect(invoke).toHaveBeenLastCalledWith('save_settings', { settings: updated })
    expect(result.current.settings).toEqual(updated)
    expect(result.current.error).toBeNull()
  })

  it('does not update state and returns false when save rejects', async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce(mockSettings)
      .mockRejectedValueOnce(new Error('disk full'))
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    const updated: Settings = { ...mockSettings, outputFolder: '/tmp/new' }
    let saved: boolean | undefined
    await act(async () => {
      saved = await result.current.saveSettings(updated)
    })
    expect(saved).toBe(false)
    expect(result.current.settings).toEqual(mockSettings)
    expect(result.current.error).toContain('Could not save settings')
    expect(result.current.error).toContain('disk full')
  })

  it('resetSettings returns and applies defaults', async () => {
    const defaults: Settings = { ...mockSettings, outputFolder: '' }
    vi.mocked(invoke)
      .mockResolvedValueOnce(mockSettings)
      .mockResolvedValueOnce(defaults)
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    let returned: Settings | null = null
    await act(async () => {
      returned = await result.current.resetSettings()
    })
    expect(invoke).toHaveBeenLastCalledWith('reset_settings')
    expect(returned).toEqual(defaults)
    expect(result.current.settings).toEqual(defaults)
    expect(result.current.error).toBeNull()
  })

  it('sets error, keeps state, returns null when reset rejects', async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce(mockSettings)
      .mockRejectedValueOnce(new Error('nope'))
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    let returned: Settings | null | undefined
    await act(async () => {
      returned = await result.current.resetSettings()
    })
    expect(returned).toBeNull()
    expect(result.current.settings).toEqual(mockSettings)
    expect(result.current.error).toContain('Could not reset settings')
    expect(result.current.error).toContain('nope')
  })

  it('browseFolder returns null when dialog cancelled', async () => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    vi.mocked(open).mockResolvedValueOnce(null)
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    let picked: string | null | undefined
    await act(async () => {
      picked = await result.current.browseFolder()
    })
    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false })
    expect(picked).toBeNull()
  })

  it('browseFolder returns selected path', async () => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    vi.mocked(open).mockResolvedValueOnce('/tmp/folder')
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    let picked: string | null | undefined
    await act(async () => {
      picked = await result.current.browseFolder()
    })
    expect(picked).toBe('/tmp/folder')
    expect(result.current.error).toBeNull()
  })

  it('browseFolder returns null for non-string selection', async () => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    vi.mocked(open).mockResolvedValueOnce(['/tmp/a', '/tmp/b'])
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    let picked: string | null | undefined
    await act(async () => {
      picked = await result.current.browseFolder()
    })
    expect(picked).toBeNull()
  })

  it('browseFolder returns null when picker throws', async () => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    vi.mocked(open).mockRejectedValueOnce(new Error('picker dead'))
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    let picked: string | null | undefined
    await act(async () => {
      picked = await result.current.browseFolder()
    })
    expect(picked).toBeNull()
    expect(result.current.error).toBeNull()
    expect(toast.error).toHaveBeenCalledWith(
      'Could not open folder picker',
      expect.objectContaining({
        id: 'settings-folder-picker',
        description: 'picker dead',
      }),
    )
    expect(consoleSpy).toHaveBeenCalledWith(
      'Folder picker failed:',
      expect.any(Error),
    )
    consoleSpy.mockRestore()
  })

  it('keeps error state and ends loading when get_settings rejects', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('load boom'))
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    expect(result.current.loading).toBe(false)
    expect(result.current.settings).toBeNull()
    expect(result.current.error).toContain('Could not load settings')
    expect(result.current.error).toContain('load boom')
  })

  it('serializes object rejections as JSON instead of [object Object]', async () => {
    vi.mocked(invoke).mockRejectedValueOnce({ code: 42 })
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    expect(result.current.error).toContain('{"code":42}')
  })

  it('uses raw string for string rejections', async () => {
    vi.mocked(invoke).mockRejectedValueOnce('plain failure')
    const { result } = renderHook(() => useSettings())
    await act(async () => {})
    expect(result.current.error).toContain('plain failure')
  })

  it('skips stale load result when save races during mount load', async () => {
    let resolveLoad!: (v: Settings) => void
    vi.mocked(invoke).mockImplementationOnce(
      () => new Promise<Settings>((r) => (resolveLoad = r)),
    )
    const { result } = renderHook(() => useSettings())
    const updated: Settings = { ...mockSettings, outputFolder: '/tmp/race' }
    vi.mocked(invoke).mockResolvedValueOnce(undefined)
    await act(async () => {
      await result.current.saveSettings(updated)
    })
    resolveLoad(mockSettings)
    await act(async () => {})
    expect(result.current.settings).toEqual(updated)
  })

  it('does not set load error when pending load rejects after save succeeds', async () => {
    let rejectLoad!: (e: unknown) => void
    vi.mocked(invoke).mockImplementationOnce(
      () => new Promise<Settings>((_, r) => (rejectLoad = r)),
    )
    const { result } = renderHook(() => useSettings())
    const updated: Settings = { ...mockSettings, outputFolder: '/tmp/race' }
    vi.mocked(invoke).mockResolvedValueOnce(undefined)
    await act(async () => {
      await result.current.saveSettings(updated)
    })
    expect(result.current.error).toBeNull()
    rejectLoad(new Error('late boom'))
    await act(async () => {})
    expect(result.current.settings).toEqual(updated)
    expect(result.current.error).toBeNull()
    expect(result.current.loading).toBe(false)
  })
})