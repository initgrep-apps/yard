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

import { useEffect, useCallback, useRef, useState, type Dispatch, type SetStateAction } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { toast } from 'sonner'
import type { Change, ConsentSource, FileEntry, FileStatus } from '@/types'
import { dropConsentForFile, clearConsentRequests, type ConsentGrant } from '@/hooks/useConsent'

interface ProgressPayload {
  fileId: string
  percent: number
}

interface CompletePayload {
  fileId: string
  outputPath: string
  originalSize: number
  compressedSize: number
  status: FileStatus
  metTarget: boolean
  strategyUsed?: string | null
  qualityUsed?: number | null
  reason?: string | null
  changes?: Change[] | null
  consentSource?: ConsentSource | null
}

interface ErrorPayload {
  fileId: string
  errorMessage: string
}

interface CancelledPayload {
  fileId: string
}

interface BatchCompletePayload {
  batchId: string
  totalSavedBytes: number
  cancelled?: boolean
  needsPermission?: number
}

interface BatchErrorPayload {
  batchId: string
  errorMessage: string
}

export interface CompressionCallbacks {
  onBatchComplete?: (
    totalSavedBytes: number,
    processedCount: number,
    errorCount: number,
    cancelled: boolean
  ) => void
  onBatchError?: (errorMessage: string) => void
}

const toMessage = (err: unknown): string =>
  err instanceof Error ? err.message : String(err)

const TERMINAL_WATCHDOG_MS = 35 * 60 * 1000

export function useCompression(
  files: FileEntry[],
  setFiles: Dispatch<SetStateAction<FileEntry[]>>,
  callbacks?: CompressionCallbacks
) {
  const [isCompressing, setIsCompressing] = useState(false)
  const [progressByFile, setProgressByFile] = useState<Record<string, number>>({})
  const processedCountRef = useRef(0)
  const errorCountRef = useRef(0)
  const cancelledRef = useRef(false)
  const activeBatchRef = useRef<string | null>(null)
  const lastTerminalRef = useRef<string | null>(null)
  const watchdogRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isCompressingRef = useRef(false)

  const filesRef = useRef(files)
  filesRef.current = files
  const callbacksRef = useRef(callbacks)
  callbacksRef.current = callbacks
  isCompressingRef.current = isCompressing

  const clearWatchdog = useCallback(() => {
    if (watchdogRef.current !== null) {
      clearTimeout(watchdogRef.current)
      watchdogRef.current = null
    }
  }, [])

  const armWatchdog = useCallback(() => {
    clearWatchdog()
    watchdogRef.current = setTimeout(() => {
      watchdogRef.current = null
      if (!isCompressingRef.current) return
      isCompressingRef.current = false
      setIsCompressing(false)
      setProgressByFile({})
      callbacksRef.current?.onBatchError?.('Compression status lost — check History for results')
      activeBatchRef.current = null
    }, TERMINAL_WATCHDOG_MS)
  }, [clearWatchdog])

  const kickWatchdog = useCallback(() => {
    if (activeBatchRef.current !== null) {
      armWatchdog()
    }
  }, [armWatchdog])

  useEffect(() => {
    let disposed = false
    const unlisteners: UnlistenFn[] = []
    const done = (u: UnlistenFn) => {
      if (disposed) {
        u()
      } else {
        unlisteners.push(u)
      }
    }
    const dropProgress = (fileId: string) =>
      setProgressByFile((prev) => {
        if (!(fileId in prev)) return prev
        const next = { ...prev }
        delete next[fileId]
        return next
      })
    const reportListenFailure = (event: string) => (err: unknown) => {
      console.error(`event listener registration failed: ${event}`, err)
      callbacksRef.current?.onBatchError?.('Lost connection to compression worker')
    }

    const clearCancelled = (payloadCancelled?: boolean) => {
      const wasCancelled = payloadCancelled ?? cancelledRef.current
      cancelledRef.current = false
      if (wasCancelled) {
        toast.info('Batch cancelled')
      }
      return wasCancelled
    }

    listen<ProgressPayload>('file:progress', (e) => {
      kickWatchdog()
      const { fileId, percent } = e.payload
      const file = filesRef.current.find((f) => f.id === fileId)
      if (!file) return
      if (file.status !== 'pending' && file.status !== 'compressing') return
      setProgressByFile((prev) => ({ ...prev, [fileId]: percent }))
      setFiles((prev) =>
        prev.map((f) =>
          f.id === fileId && f.status === 'pending' ? { ...f, status: 'compressing' } : f
        )
      )
    })
      .then(done)
      .catch(reportListenFailure('file:progress'))

    listen<CompletePayload>('file:complete', (e) => {
      kickWatchdog()
      const {
        fileId,
        outputPath,
        compressedSize,
        status,
        strategyUsed = null,
        qualityUsed = null,
        reason = null,
      } = e.payload
      if (filesRef.current.some((f) => f.id === fileId)) {
        processedCountRef.current += 1
      }
      dropConsentForFile(fileId)
      dropProgress(fileId)
      const denied = status === 'needs_permission'
      setFiles((prev) =>
        prev.map((f) =>
          f.id === fileId
            ? {
                ...f,
                status,
                compressedSize: denied ? null : compressedSize,
                outputPath: denied ? null : outputPath,
                strategyUsed,
                qualityUsed,
                error: denied ? reason : f.error,
              }
            : f
        )
      )
    })
      .then(done)
      .catch(reportListenFailure('file:complete'))

    listen<ErrorPayload>('file:error', (e) => {
      kickWatchdog()
      const { fileId, errorMessage } = e.payload
      if (filesRef.current.some((f) => f.id === fileId)) {
        processedCountRef.current += 1
        errorCountRef.current += 1
      }
      dropConsentForFile(fileId)
      dropProgress(fileId)
      setFiles((prev) =>
        prev.map((f) =>
          f.id === fileId
            ? { ...f, status: 'failed', error: errorMessage, strategyUsed: null, qualityUsed: null }
            : f
        )
      )
    })
      .then(done)
      .catch(reportListenFailure('file:error'))

    listen<CancelledPayload>('file:cancelled', (e) => {
      kickWatchdog()
      const { fileId } = e.payload
      dropConsentForFile(fileId)
      dropProgress(fileId)
      setFiles((prev) =>
        prev.map((f) =>
          f.id === fileId
            ? { ...f, status: 'cancelled', strategyUsed: null, qualityUsed: null }
            : f
        )
      )
    })
      .then(done)
      .catch(reportListenFailure('file:cancelled'))

    const staleBatchEvent = (batchId: string) =>
      activeBatchRef.current !== null && batchId !== '' && batchId !== activeBatchRef.current

    listen<BatchCompletePayload>('batch:complete', (e) => {
      if (staleBatchEvent(e.payload.batchId)) return
      if (e.payload.batchId === lastTerminalRef.current) return
      lastTerminalRef.current = e.payload.batchId
      activeBatchRef.current = null
      clearWatchdog()
      clearConsentRequests()
      setIsCompressing(false)
      isCompressingRef.current = false
      const processed = processedCountRef.current
      const errors = errorCountRef.current
      processedCountRef.current = 0
      errorCountRef.current = 0
      const cancelled = clearCancelled(e.payload.cancelled)
      const needsPermission = e.payload.needsPermission ?? 0
      if (needsPermission > 0) {
        toast.info(
          `${needsPermission} files need permission — review them before retrying`
        )
      }
      if (processed > 0 || cancelled) {
        callbacksRef.current?.onBatchComplete?.(
          e.payload.totalSavedBytes,
          processed,
          errors,
          cancelled
        )
      }
    })
      .then(done)
      .catch(reportListenFailure('batch:complete'))

    listen<BatchErrorPayload>('batch:error', (e) => {
      if (staleBatchEvent(e.payload.batchId)) return
      if (e.payload.batchId === lastTerminalRef.current) return
      lastTerminalRef.current = e.payload.batchId
      activeBatchRef.current = null
      clearWatchdog()
      clearConsentRequests()
      setIsCompressing(false)
      isCompressingRef.current = false
      processedCountRef.current = 0
      errorCountRef.current = 0
      clearCancelled()
      callbacksRef.current?.onBatchError?.(e.payload.errorMessage)
    })
      .then(done)
      .catch(reportListenFailure('batch:error'))

    return () => {
      disposed = true
      clearWatchdog()
      unlisteners.forEach((u) => u())
    }
  }, [setFiles, clearWatchdog, kickWatchdog])

  const compressAll = useCallback(async (grants?: ConsentGrant[]) => {
    const pendingCount = filesRef.current.filter((f) => f.status === 'pending').length
    if (pendingCount === 0) return
    clearWatchdog()
    isCompressingRef.current = true
    setIsCompressing(true)
    cancelledRef.current = false
    try {
      const batchId =
        grants && grants.length > 0
          ? await invoke<string>('compress_all', { grants })
          : await invoke<string>('compress_all')
      activeBatchRef.current = typeof batchId === 'string' && batchId !== '' ? batchId : null
      lastTerminalRef.current = null
      if (typeof batchId === 'string' && batchId !== '') {
        armWatchdog()
      }
      if (batchId === '') {
        setIsCompressing(false)
        isCompressingRef.current = false
        callbacksRef.current?.onBatchError?.('No pending files')
      }
    } catch (err) {
      activeBatchRef.current = null
      lastTerminalRef.current = null
      clearWatchdog()
      setIsCompressing(false)
      isCompressingRef.current = false
      callbacksRef.current?.onBatchError?.(toMessage(err))
    }
  }, [armWatchdog, clearWatchdog])

  const cancelFile = useCallback(async (fileId: string) => {
    try {
      await invoke('cancel_file', { fileId })
    } catch (err) {
      toast.error(`Could not cancel file: ${toMessage(err)}`)
    }
  }, [])

  const cancelAll = useCallback(async () => {
    try {
      await invoke('cancel_all')
      cancelledRef.current = true
    } catch (err) {
      toast.error(`Could not cancel: ${toMessage(err)}`)
    }
  }, [])

  const revealInFolder = useCallback(async (filePath: string) => {
    try {
      await invoke('reveal_in_folder', { filePath })
    } catch (err) {
      toast.error(`Could not open folder: ${toMessage(err)}`)
    }
  }, [])

  return { isCompressing, progressByFile, compressAll, cancelFile, cancelAll, revealInFolder }
}
