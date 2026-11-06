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
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { toast } from 'sonner'
import { useCompressionContext } from '@/providers/CompressionProvider'
import { DropZone } from '@/components/DropZone'
import { FileList } from '@/components/FileList'
import { formatSize } from '@/lib/sizeFormat'
import { RunHeader } from '@/components/run/RunHeader'
import { RunBoard } from '@/components/run/RunBoard'
import { PreflightDialog } from '@/components/run/PreflightDialog'
import { useConsent } from '@/hooks/useConsent'
import { batchCounts, type StatusFilter } from '@/lib/grouping'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter } from '@/components/ui/card'

const BATCH_LOCKED_MESSAGE = 'Batch in progress — wait until it completes'

function useBatchLockedDrops() {
  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | undefined

    let subscription: Promise<() => void>
    try {
      subscription = getCurrentWebview().onDragDropEvent((event) => {
        if (cancelled) return
        const payload = event.payload
        if (payload.type === 'drop' && payload.paths.length > 0) {
          toast.info(BATCH_LOCKED_MESSAGE)
        }
      })
    } catch (err) {
      subscription = Promise.reject(err)
    }

    subscription
      .then((fn) => {
        if (cancelled) fn()
        else unlisten = fn
      })
      .catch((err) => {
        console.error('Drag-drop listener registration failed', err)
        toast.error('Drop notifications unavailable — restart the app to re-enable dragging')
      })

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])
}

function RunMonitor() {
  const {
    files,
    isCompressing,
    progressByFile,
    cancelFile,
    cancelAll,
    revealInFolder,
    removeFile,
    batchNumber,
    roster,
    batchError,
    batchCancelled,
    endRun,
  } = useCompressionContext()
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all')
  const { waitingFileIds } = useConsent()
  useBatchLockedDrops()

  const rosterFiles = files.filter((f) => roster.includes(f.id))
  const counts = batchCounts(rosterFiles, waitingFileIds)
  const total = rosterFiles.length

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-hidden p-5 min-w-0">
      <RunHeader
        batchNumber={batchNumber}
        running={isCompressing}
        batchError={batchError}
        cancelled={batchCancelled}
        counts={counts}
        total={total}
        statusFilter={statusFilter}
        onFilterChange={setStatusFilter}
        onCancelAll={() => void cancelAll()}
        onNewBatch={endRun}
      />
      <div className="min-h-0 flex-1 overflow-hidden">
        <RunBoard
          roster={rosterFiles}
          statusFilter={statusFilter}
          onFilterChange={setStatusFilter}
          progress={progressByFile}
          onCancel={(fileId) => void cancelFile(fileId)}
          onReveal={(outputPath) => void revealInFolder(outputPath)}
          onRemove={(fileId) => void removeFile(fileId)}
          removeDisabled={isCompressing}
        />
      </div>
    </div>
  )
}

function StagingContentCard() {
  const {
    files,
    isCompressing,
    removeFile,
    clearList,
    revealInFolder,
    startRun,
  } = useCompressionContext()

  const compressible = files.filter((f) => f.status !== 'unsupported')
  const pendingCount = files.filter((f) => f.status === 'pending').length
  const canCompress = pendingCount > 0
  const totalSize = compressible.reduce((sum, f) => sum + f.originalSize, 0)

  return (
    <Card className="flex min-h-0 flex-1 flex-col">
      <CardContent className="min-h-0 flex-1 overflow-hidden">
        <FileList
          files={files}
          onRemove={removeFile}
          onReveal={revealInFolder}
          removeDisabled={isCompressing}
        />
      </CardContent>
      <CardFooter className="justify-between gap-2 border-t">
        <div
          data-testid="batch-summary"
          className="text-xs text-muted-foreground tabular-nums"
        >
          {compressible.length} files selected · Total size: ~{formatSize(totalSize)}
        </div>
        <div className="flex gap-2">
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void clearList()}
            disabled={isCompressing}
          >
            Clear List
          </Button>
          <Button size="sm" onClick={() => startRun()} disabled={!canCompress || isCompressing}>
            Compress
          </Button>
        </div>
      </CardFooter>
    </Card>
  )
}

function StagingScreen() {
  const {
    files,
    error,
    batchError,
    addFiles,
    browseFiles,
  } = useCompressionContext()

  const alert = (message: string) => (
    <div
      role="alert"
      className="shrink-0 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive"
    >
      {message}
    </div>
  )

  if (files.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-hidden p-5">
        {error ? alert(error) : null}
        {batchError ? alert(batchError) : null}
        <div className="flex min-h-0 flex-1 items-center justify-center">
          <DropZone onBrowse={browseFiles} onDrop={addFiles} />
        </div>
      </div>
    )
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-hidden p-5">
      {error ? alert(error) : null}
      {batchError ? alert(batchError) : null}

      <DropZone compact onBrowse={browseFiles} onDrop={addFiles} className="shrink-0" />

      <StagingContentCard />
    </div>
  )
}

export function CompressView() {
  const { runScreen } = useCompressionContext()
  return (
    <>
      {runScreen === 'run' ? <RunMonitor /> : <StagingScreen />}
      <PreflightDialog />
    </>
  )
}
