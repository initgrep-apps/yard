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

import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from 'react'
import { useFileList } from '@/hooks/useFileList'
import { useCompression } from '@/hooks/useCompression'
import { requestPreflight } from '@/hooks/useConsent'
import { toast } from 'sonner'
import { formatSize } from '@/lib/sizeFormat'
import type { FileEntry } from '@/types'

function formatSavedBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  return formatSize(bytes)
}

export type RunScreen = 'staging' | 'run'

export interface CompressionContextValue {
  files: ReturnType<typeof useFileList>['files']
  error: ReturnType<typeof useFileList>['error']
  addFiles: ReturnType<typeof useFileList>['addFiles']
  browseFiles: ReturnType<typeof useFileList>['browseFiles']
  removeFile: ReturnType<typeof useFileList>['removeFile']
  clearList: ReturnType<typeof useFileList>['clearList']
  isCompressing: ReturnType<typeof useCompression>['isCompressing']
  progressByFile: ReturnType<typeof useCompression>['progressByFile']
  cancelFile: ReturnType<typeof useCompression>['cancelFile']
  cancelAll: ReturnType<typeof useCompression>['cancelAll']
  revealInFolder: ReturnType<typeof useCompression>['revealInFolder']
  runScreen: RunScreen
  batchNumber: number
  batchError: string | null
  batchCancelled: boolean
  roster: Array<FileEntry['id']>
  startRun: () => void
  endRun: () => void
}

export const CompressionContext = createContext<CompressionContextValue | null>(null)

export function CompressionProvider({ children }: { children: ReactNode }) {
  const { files, error, addFiles, browseFiles, removeFile, clearList, setFiles } = useFileList()
  const { isCompressing, progressByFile, compressAll, cancelFile, cancelAll, revealInFolder } =
    useCompression(files, setFiles, {
      onBatchComplete: (saved, count, errorCount, cancelled) => {
        if (count > 0) {
          toast.success(`Compressed ${count} files. Saved ${formatSavedBytes(saved)} total.`)
        }
        if (errorCount > 0) toast.error(`${errorCount} files could not reach target.`)
        setBatchError(null)
        setBatchCancelled(cancelled)
      },
      onBatchError: (message) => {
        toast.error(message)
        setBatchError(message)
      },
    })

  const filesRef = useRef(files)
  filesRef.current = files
  const [runScreen, setRunScreen] = useState<RunScreen>('staging')
  const [batchNumber, setBatchNumber] = useState(0)
  const [batchError, setBatchError] = useState<string | null>(null)
  const [batchCancelled, setBatchCancelled] = useState(false)
  const [roster, setRoster] = useState<Array<FileEntry['id']>>([])

  const startRun = useCallback(async () => {
    if (isCompressing) return
    const pending = filesRef.current.filter((f) => f.status === 'pending')
    if (pending.length === 0) return
    const grants = await requestPreflight(pending)
    if (grants === null) return
    setRoster(pending.map((f) => f.id))
    setBatchNumber((n) => n + 1)
    setBatchCancelled(false)
    setRunScreen('run')
    void compressAll(grants.length > 0 ? grants : undefined)
  }, [compressAll, isCompressing])

  const endRun = useCallback(() => {
    void clearList()
    setRunScreen('staging')
  }, [clearList])

  const value: CompressionContextValue = {
    files,
    error,
    addFiles,
    browseFiles,
    removeFile,
    clearList,
    isCompressing,
    progressByFile,
    cancelFile,
    cancelAll,
    revealInFolder,
    runScreen,
    batchNumber,
    batchError,
    batchCancelled,
    roster,
    startRun,
    endRun,
  }

  return (
    <CompressionContext.Provider value={value}>{children}</CompressionContext.Provider>
  )
}

export function useCompressionContext(): CompressionContextValue {
  const ctx = useContext(CompressionContext)
  if (!ctx) {
    throw new Error('useCompressionContext must be used within CompressionProvider')
  }
  return ctx
}
