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

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export interface MemoryStats {
  totalRamBytes: number
  budgetBytes: number
  processRssBytes: number
}

export interface BatchMemoryUsage {
  budgetBytes: number
  usedBytes: number
  processRssBytes: number
}

export function useMemory() {
  const [stats, setStats] = useState<MemoryStats | null>(null)
  const [batchUsage, setBatchUsage] = useState<BatchMemoryUsage | null>(null)

  useEffect(() => {
    let cancelled = false
    const load = async () => {
      try {
        const s = await invoke<MemoryStats>('memory_stats')
        if (!cancelled) setStats(s)
      } catch (err) {
        console.error('memory_stats failed:', err)
      }
    }
    load()
    const id = window.setInterval(load, 2000)
    return () => {
      cancelled = true
      window.clearInterval(id)
    }
  }, [])

  useEffect(() => {
    const unlisteners: Array<() => void> = []
    let disposed = false
    const attach = <T,>(event: string, handler: (payload: T) => void) => {
      listen<T>(event, (e) => handler(e.payload))
        .then((fn) => {
          if (disposed) fn()
          else unlisteners.push(fn)
        })
        .catch((err) => console.error(`${event} listen failed:`, err))
    }
    attach<BatchMemoryUsage>('memory:usage', setBatchUsage)
    attach<unknown>('batch:complete', () => setBatchUsage(null))
    attach<unknown>('batch:error', () => setBatchUsage(null))
    return () => {
      disposed = true
      unlisteners.forEach((fn) => fn())
    }
  }, [])

  return { stats, batchUsage }
}
