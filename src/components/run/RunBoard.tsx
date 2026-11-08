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

import { useEffect, useMemo, useRef, useState } from 'react'
import type { ReactElement } from 'react'
import { File, FileQuestion, FileText, Film, GripVerticalIcon, Image, type LucideIcon } from 'lucide-react'
import { RunCard } from '@/components/run/RunCard'
import { Button } from '@/components/ui/button'
import { Kanban, KanbanBoard, KanbanColumn, KanbanColumnContent, KanbanColumnHandle, KanbanOverlay } from '@/components/ui/kanban'
import { dragAxisModifiers } from '@/lib/boardModifiers'
import { applyCardMove, applyLaneOrder, derivedColumns, initialBoardState, syncBoardState, type BoardState } from '@/lib/boardState'
import { LANE_ORDER, laneDonePill, type StatusFilter } from '@/lib/grouping'
import { CATEGORY_TONES } from '@/lib/tokens'
import { cn } from '@/lib/utils'
import type { Category, FileEntry } from '@/types'

export interface RunBoardProps {
  roster: FileEntry[]
  statusFilter: StatusFilter
  onFilterChange: (f: StatusFilter) => void
  progress?: Record<string, number>
  onCancel: (fileId: string) => void
  onReveal: (path: string) => void
  onRemove: (fileId: string) => void
  removeDisabled?: boolean
}

const EMPTY_WORDS: Record<Exclude<StatusFilter, 'all'>, string> = {
  compressing: 'processing',
  complete: 'complete',
  below_target: 'below target',
  failed: 'failed',
  queued: 'queued',
}

const LANE_LABELS: Record<Category, string> = {
  image: 'Image',
  video: 'Video',
  pdf: 'PDF',
  document: 'Document',
  other: 'Other',
}

const LANE_ICONS: Record<Category, LucideIcon> = {
  image: Image,
  video: Film,
  pdf: FileText,
  document: File,
  other: FileQuestion,
}

const PILL_BASE = 'ml-auto rounded-full px-2 py-[2.5px] font-mono text-[10px] font-semibold tabular-nums'

function laneOf(category: Category): Category {
  return LANE_ORDER.includes(category) ? category : 'other'
}

interface LaneViewProps {
  category: Category
  files: FileEntry[]
  laneRoster: FileEntry[]
  progress?: Record<string, number>
  onCancel: (fileId: string) => void
  onReveal: (path: string) => void
  onRemove?: (fileId: string) => void
  isOverlay?: boolean
  ghostHeight?: number
}

function LaneView({ category, files, laneRoster, progress, onCancel, onReveal, onRemove, isOverlay, ghostHeight }: LaneViewProps) {
  const tone = CATEGORY_TONES[category]
  const Icon = LANE_ICONS[category]
  const pill = laneDonePill(laneRoster)
  const live = laneRoster.some((file) => file.status === 'compressing')

  return (
    <KanbanColumn
      value={category}
      className={isOverlay ? 'w-64' : 'h-full min-h-0'}
      style={isOverlay && ghostHeight ? { height: ghostHeight } : undefined}
    >
      <div className={cn('flex flex-col', !isOverlay && 'h-full')}>
        <div className="flex items-center gap-[9px] border-b border-border/65 px-3 py-[11px]">
          <div className={cn('flex size-6 shrink-0 items-center justify-center rounded-md border', tone.tint, tone.text, tone.border)}>
            <Icon className="size-[13px]" />
          </div>
          <span className="text-[13px] font-semibold tracking-[0.01em] text-foreground">{LANE_LABELS[category]}</span>
          <span
            data-testid="lane-pill"
            className={cn(PILL_BASE, live ? 'bg-primary/10 text-primary' : 'bg-muted text-muted-foreground')}
          >
            {pill.done}/{pill.total} done
          </span>
          <KanbanColumnHandle className="opacity-100">
            <Button size="icon-xs" variant="ghost" aria-label={`Reorder ${LANE_LABELS[category]} lane`}>
              <GripVerticalIcon />
            </Button>
          </KanbanColumnHandle>
        </div>
        <KanbanColumnContent value={category} className="mt-2 min-h-0 flex-1 gap-2.5 overflow-y-auto pt-2 pl-3 pr-3.5 pb-3" data-lane-scroll>
          {files.map((file) => (
            <RunCard
              key={file.id}
              file={file}
              percent={progress?.[file.id]}
              onCancel={onCancel}
              onReveal={onReveal}
              onRemove={onRemove}
              isOverlay={isOverlay}
            />
          ))}
        </KanbanColumnContent>
      </div>
    </KanbanColumn>
  )
}

export function RunBoard({
  roster,
  statusFilter,
  onFilterChange,
  progress,
  onCancel,
  onReveal,
  onRemove,
  removeDisabled,
}: RunBoardProps): ReactElement {
  const [state, setState] = useState<BoardState>(() => initialBoardState(roster))
  const stateRef = useRef(state)
  stateRef.current = state
  const rosterRef = useRef(roster)
  rosterRef.current = roster
  const rosterKey = useMemo(() => roster.map((file) => file.id).join(','), [roster])
  const compressKey = useMemo(
    () =>
      roster
        .filter((file) => file.status === 'compressing')
        .map((file) => file.id)
        .join(','),
    [roster]
  )
  const boardRef = useRef<HTMLDivElement | null>(null)
  const [laneHeight, setLaneHeight] = useState(0)

  useEffect(() => {
    const next = syncBoardState(stateRef.current, rosterRef.current)
    if (next !== stateRef.current) setState(next)
  }, [rosterKey, compressKey])

  const columnsList = useMemo(() => derivedColumns(state, roster, statusFilter), [state, roster, statusFilter])
  const columns = useMemo(() => Object.fromEntries(columnsList.map((c) => [c.category, c.files])), [columnsList])
  const hasLanes = columnsList.length !== 0

  const modifiers = useMemo(() => dragAxisModifiers((id) => columnsList.some((c) => c.category === id)), [columnsList])

  useEffect(() => {
    const wrapper = boardRef.current
    if (!wrapper || typeof window.ResizeObserver === 'undefined') return
    const firstLane = wrapper.querySelector('[data-slot="kanban-column"]')
    if (!firstLane) return
    const observer = new ResizeObserver(() => setLaneHeight(firstLane.clientHeight))
    observer.observe(firstLane)
    return () => observer.disconnect()
  }, [hasLanes])

  return (
    <Kanban
      value={columns}
      className="h-full min-h-0"
      modifiers={modifiers}
      onValueChange={(value) => setState((prev) => applyLaneOrder(prev, Object.keys(value)))}
      getItemValue={(file) => file.id}
      onMove={({ event, activeContainer, overContainer }) => {
        const { active, over } = event
        setState((prev) =>
          applyCardMove(
            prev,
            {
              activeId: String(active.id),
              overId: over ? String(over.id) : '',
              activeContainer,
              overContainer,
              overIsColumn: over ? Object.keys(columns).includes(String(over.id)) : false,
            },
            roster
          )
        )
      }}
    >
      {columnsList.length === 0 ? (
        <div data-testid="board-empty" className="flex min-h-[240px] flex-col items-center justify-center gap-1.5">
          <p className="text-[13px] text-muted-foreground">
            Nothing {statusFilter === 'all' ? 'here' : EMPTY_WORDS[statusFilter]} right now
          </p>
          <button
            type="button"
            data-testid="show-all"
            className="cursor-pointer text-[12.5px] text-primary underline-offset-2 hover:underline"
            onClick={() => onFilterChange('all')}
          >
            Show all
          </button>
        </div>
      ) : (
        <div
          ref={boardRef}
          data-testid="kanban-board"
          className="h-full overflow-x-auto select-none pb-3 pr-6"
        >
          <KanbanBoard
            className="h-full w-max min-w-full gap-3 auto-rows-fr"
            style={{ gridTemplateColumns: `repeat(${columnsList.length}, 16rem)` }}
          >
            {columnsList.map((column) => (
              <LaneView
                key={column.category}
                category={column.category}
                files={column.files}
                laneRoster={roster.filter((file) => laneOf(file.category) === column.category)}
                progress={progress}
                onCancel={onCancel}
                onReveal={onReveal}
                onRemove={removeDisabled ? undefined : onRemove}
              />
            ))}
          </KanbanBoard>
        </div>
      )}
      <KanbanOverlay>
        {({ value, variant }) => {
          if (variant === 'column') {
            const category = String(value) as Category
            return (
              <LaneView
                category={category}
                files={columns[String(value)]}
                laneRoster={roster.filter((file) => laneOf(file.category) === category)}
                progress={progress}
                onCancel={onCancel}
                onReveal={onReveal}
                onRemove={removeDisabled ? undefined : onRemove}
                isOverlay
                ghostHeight={laneHeight}
              />
            )
          }
          const file = roster.find((entry) => entry.id === value)
          return file ? <RunCard file={file} percent={progress?.[file.id]} isOverlay /> : null
        }}
      </KanbanOverlay>
    </Kanban>
  )
}
