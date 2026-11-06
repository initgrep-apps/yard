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

import { FolderOpen, Square, X } from 'lucide-react'
import { formatSize, reductionPercentOrNull } from '@/lib/sizeFormat'
import { RESULT_STATUSES } from '@/components/FileList'
import { ConsentPrompt } from '@/components/run/ConsentPrompt'
import { useConsent } from '@/hooks/useConsent'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { KanbanItem, KanbanItemHandle } from '@/components/ui/kanban'
import { Progress } from '@/components/ui/progress'
import { Separator } from '@/components/ui/separator'
import { Spinner } from '@/components/ui/spinner'
import { cn } from '@/lib/utils'
import { strategyLabel } from '@/lib/strategyLabels'
import type { FileEntry } from '@/types'

export interface RunCardProps {
  file: FileEntry
  percent?: number
  onCancel?: (fileId: string) => void
  onReveal?: (outputPath: string) => void
  onRemove?: (fileId: string) => void
  isOverlay?: boolean
}

type ChipTone = 'primary' | 'amber' | 'fail' | 'muted'

const CHIP_TONES: Record<ChipTone, string> = {
  primary: 'bg-primary/10 text-primary',
  amber: 'bg-amber-500/10 text-amber-600',
  fail: 'bg-destructive/10 text-destructive',
  muted: 'bg-muted text-muted-foreground',
}

function safePercent(percent: number | undefined): number {
  return percent !== undefined && Number.isFinite(percent)
    ? Math.min(100, Math.max(0, percent))
    : 0
}

function chipFor(file: FileEntry, waiting: boolean): { label: string; tone: ChipTone } | null {
  if (waiting) return { label: 'Waiting', tone: 'amber' }
  switch (file.status) {
    case 'done':
    case 'best_effort': {
      const pct = reductionPercentOrNull(file)
      if (pct === null) return null
      return { label: `-${pct}%`, tone: file.status === 'done' ? 'primary' : 'amber' }
    }
    case 'already_under_target':
      return { label: '≤ target', tone: 'muted' }
    case 'failed':
      return { label: 'Failed', tone: 'fail' }
    case 'needs_permission':
      return { label: 'Permission', tone: 'amber' }
    case 'cancelled':
      return { label: 'Cancelled', tone: 'muted' }
    case 'pending':
      return { label: 'Queued', tone: 'muted' }
    case 'compressing':
      return { label: 'Processing', tone: 'primary' }
    default:
      return null
  }
}

function sizesFor(file: FileEntry): string {
  switch (file.status) {
    case 'compressing':
      return `${formatSize(file.originalSize)} → …`
    case 'already_under_target':
      return formatSize(file.originalSize)
    case 'failed':
      return ''
    case 'needs_permission':
      return ''
    default:
      return file.compressedSize === null
        ? formatSize(file.originalSize)
        : `${formatSize(file.originalSize)} → ${formatSize(file.compressedSize)}`
  }
}

function strategyLineFor(file: FileEntry): string {
  if (file.status === 'already_under_target') {
    return `Already under ${formatSize(file.targetBytes)}`
  }
  const label = strategyLabel(file.strategyUsed, file.category)
  if (file.status === 'best_effort') {
    if (!label || label === 'Best effort') return 'Best effort'
    return `Best effort · ${label}`
  }
  return label
}

export function RunCard({
  file,
  percent,
  onCancel,
  onReveal,
  onRemove,
  isOverlay,
}: RunCardProps) {
  const { status, outputPath, error } = file
  const { requestFor, answerConsent } = useConsent()
  const consentRequest = requestFor(file.id)
  const clampedPercent = safePercent(percent)
  const tone = chipFor(file, consentRequest !== null)
  const sizes = sizesFor(file)
  const note = strategyLineFor(file)
  const hasResult = RESULT_STATUSES.includes(status)

  const card = (
    <Card
      size="sm"
      className={cn(
        status === 'failed' && 'border-destructive/25 bg-destructive/5',
        (status === 'needs_permission' || consentRequest) &&
          'border-amber-500/25 bg-amber-500/5',
        status === 'pending' && 'border-dashed',
        status === 'cancelled' && 'opacity-60',
        isOverlay && 'ring-primary/20 rotate-1 ring-2'
      )}
    >
      <CardContent className="space-y-3">
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs font-semibold truncate" title={file.name}>
            {file.name}
          </span>
          <div className="flex items-center gap-1">
            {!isOverlay && status === 'compressing' && onCancel && (
              <Button
                size="icon-xs"
                variant="ghost"
                aria-label={`Cancel ${file.name}`}
                onClick={() => onCancel(file.id)}
              >
                <Square className="size-3" />
              </Button>
            )}
            {!isOverlay && hasResult && onReveal && outputPath && (
              <Button
                size="icon-xs"
                variant="ghost"
                aria-label={`Reveal ${file.name}`}
                title="Reveal in Finder"
                onClick={() => onReveal(outputPath)}
              >
                <FolderOpen className="size-3" />
              </Button>
            )}
            {!isOverlay && status === 'pending' && onRemove && (
              <Button
                size="icon-xs"
                variant="ghost"
                aria-label={`Remove ${file.name}`}
                onClick={() => onRemove(file.id)}
              >
                <X className="size-3" />
              </Button>
            )}
          </div>
        </div>
        {note && (
          <Badge variant="secondary" className="rounded">
            {note}
          </Badge>
        )}
        {consentRequest ? (
          <ConsentPrompt request={consentRequest} onAnswer={answerConsent} />
        ) : (
          status === 'compressing' && (
            <div className="space-y-1">
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <Spinner className="size-3" />
                  <span className="text-xs font-medium">Progress</span>
                </div>
                <span
                  className="text-xs font-medium tabular-nums"
                  data-testid="run-card-percent"
                >
                  {Math.round(clampedPercent)}%
                </span>
              </div>
              <Progress value={clampedPercent} />
              <div className="font-mono text-xs tabular-nums text-muted-foreground">
                {sizes}
              </div>
            </div>
          )
        )}
        <Separator />
        <div className="flex items-center justify-between gap-2">
          {tone ? (
            <Badge
              data-testid="run-chip"
              className={cn('font-mono text-xs', CHIP_TONES[tone.tone])}
            >
              {tone.label}
            </Badge>
          ) : (
            <span />
          )}
          {status !== 'compressing' && (
            <span className="font-mono text-xs tabular-nums text-muted-foreground">
              {status === 'failed' || status === 'needs_permission' ? '—' : sizes}
            </span>
          )}
        </div>
        {(status === 'failed' || status === 'best_effort' || status === 'needs_permission') && error && (
          <p className="line-clamp-2 text-xs text-destructive" title={error}>
            {error}
          </p>
        )}
      </CardContent>
    </Card>
  )

  if (isOverlay) return card

  const content = status === 'compressing' ? card : <KanbanItemHandle>{card}</KanbanItemHandle>

  return <KanbanItem value={file.id}>{content}</KanbanItem>
}
