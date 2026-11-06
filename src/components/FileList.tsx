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

import type { FileEntry, Category, FileStatus } from '@/types'
import { formatSize, reductionPercentOrZero } from '@/lib/sizeFormat'
import { CATEGORY_TONES, STATUS_TONES } from '@/lib/tokens'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Badge } from '@/components/ui/badge'

interface FileListProps {
  files: FileEntry[]
  onRemove: (fileId: string) => void
  onReveal?: (outputPath: string) => void
  removeDisabled?: boolean
}

export const RESULT_STATUSES: FileStatus[] = ['done', 'best_effort', 'already_under_target']

const categoryLabel: Record<Category, string> = {
  image: 'Image',
  video: 'Video',
  pdf: 'PDF',
  document: 'Document',
  other: 'Other',
}

function StatusBadge({ status }: { status: FileStatus }) {
  const labels: Record<FileStatus, string> = {
    pending: 'Pending',
    compressing: 'Compressing',
    done: 'Done',
    best_effort: 'Best effort',
    failed: 'Failed',
    cancelled: 'Cancelled',
    unsupported: 'Unsupported',
    already_under_target: 'Already under target',
    needs_permission: 'Needs permission',
  }
  const tone = STATUS_TONES[status]
  return (
    <span className={`flex items-center gap-1.5 text-xs ${tone.text}`}>
      <span className={`w-1.5 h-1.5 rounded-full ${tone.accent}`} />
      {labels[status]}
    </span>
  )
}

export function FileList({ files, onRemove, onReveal, removeDisabled }: FileListProps) {
  if (files.length === 0) return null

  return (
    <div
      data-slot="file-list-scroll"
      className="h-full overflow-y-auto bg-card [&>[data-slot=table-container]]:overflow-visible"
    >
      <Table>
        <TableHeader className="sticky top-0 z-10 bg-card">
          <TableRow>
            <TableHead className="w-[40%]">File</TableHead>
            <TableHead>Category</TableHead>
            <TableHead>Target</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="w-8"></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {files.map((file) => {
            const hasResult =
              RESULT_STATUSES.includes(file.status) && file.compressedSize !== null
            const canReveal = hasResult && !!onReveal && !!file.outputPath
            const tone = CATEGORY_TONES[file.category]
            return (
              <TableRow
                key={file.id}
                className={`group ${file.status === 'unsupported' ? 'opacity-40' : ''}`}
              >
                <TableCell>
                  <div className="text-sm font-medium truncate max-w-[200px]">{file.name}</div>
                  {hasResult ? (
                    <div
                      className={`text-xs mt-0.5 tabular-nums ${
                        file.status === 'best_effort' ? STATUS_TONES.best_effort.text : 'text-muted-foreground'
                      }`}
                    >
                      {formatSize(file.originalSize)} → {formatSize(file.compressedSize!)} ·{' '}
                      {reductionPercentOrZero(file)}% smaller
                    </div>
                  ) : (
                    <div className="text-xs text-muted-foreground mt-0.5 tabular-nums">
                      {formatSize(file.originalSize)}
                    </div>
                  )}
                </TableCell>
                <TableCell>
                  <Badge
                    variant="secondary"
                    className={`${tone.border} ${tone.tint} ${tone.text}`}
                  >
                    {categoryLabel[file.category]}
                  </Badge>
                </TableCell>
                <TableCell className="text-xs text-muted-foreground tabular-nums">
                  {formatSize(file.targetBytes)}
                </TableCell>
                <TableCell>
                  <div className="flex flex-col gap-1.5">
                    <StatusBadge status={file.status} />
                    {(file.status === 'failed' || file.status === 'needs_permission') && file.error && (
                      <div className="text-xs text-destructive max-w-[180px] truncate" title={file.error}>
                        {file.error}
                      </div>
                    )}
                  </div>
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-0.5">
                    {canReveal && (
                      <button
                        onClick={() => onReveal?.(file.outputPath!)}
                        aria-label={`Reveal ${file.name}`}
                        title="Reveal in folder"
                        className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted transition-colors opacity-0 group-hover:opacity-100 focus:opacity-100"
                      >
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                          <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                        </svg>
                      </button>
                    )}
                    <button
                      onClick={() => onRemove(file.id)}
                      disabled={removeDisabled && file.status === 'pending'}
                      aria-label={`Remove ${file.name}`}
                      className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted transition-colors disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:text-muted-foreground disabled:hover:bg-transparent"
                      title="Remove from list"
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                        <line x1="18" y1="6" x2="6" y2="18" />
                        <line x1="6" y1="6" x2="18" y2="18" />
                      </svg>
                    </button>
                  </div>
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}
