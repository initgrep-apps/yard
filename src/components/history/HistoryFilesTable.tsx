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

import {
  Fragment,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from "react"
import {
  createSortedRowModel,
  flexRender,
  rowSortingFeature,
  tableFeatures,
  useTable,
  type Column,
  type ColumnDef,
  type Row,
  type SortingState,
} from "@tanstack/react-table"
import { useVirtualizer } from "@tanstack/react-virtual"
import { invoke } from "@tauri-apps/api/core"
import { toast } from "sonner"
import {
  ArrowDown,
  ArrowUp,
  ChevronDown,
  ChevronRight,
  ChevronsUpDown,
  Ellipsis,
  Folder,
  FolderOpen,
} from "lucide-react"
import type { FileStatus, HistoryFile, HistorySession } from "@/types"
import {
  HISTORY_STATUS_LABELS,
  HISTORY_STATUS_ORDER,
  HISTORY_STATUS_TONES,
  currentSizeBytes,
  currentSizeLabel,
  filterHistoryFiles,
  historyOutcomeCounts,
  shouldVirtualizeRows,
  virtualItemKey,
  type HistoryOutcomeFilter,
  type HistoryTypeFilter,
} from "@/lib/historyTable"
import { CATEGORY_TONES } from "@/lib/tokens"
import { cn } from "@/lib/utils"
import { CategoryIcon } from "@/components/CategoryIcon"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { HistoryFileDetail } from "@/components/history/HistoryFileDetail"
import { HistoryTableToolbar } from "@/components/history/HistoryTableToolbar"

const features = tableFeatures({
  rowSortingFeature,
  sortedRowModel: createSortedRowModel(),
})

type HistoryRow = Row<typeof features, HistoryFile>

const HEADER_LABELS: Record<string, string> = {
  file: "File",
  size: "Size",
  status: "Status",
}

function toastOpenError(err: unknown, prefix: string) {
  const message = String(err)
  const alreadyLabeled = /^Could not (reveal file|open folder)/.test(message)
  toast.error(alreadyLabeled ? message : `${prefix}: ${message}`)
}

async function revealFile(filePath: string) {
  try {
    await invoke("reveal_in_folder", { filePath })
  } catch (err) {
    console.error("reveal_in_folder failed:", err)
    toastOpenError(err, "Could not reveal file")
  }
}

function ariaSort(column: Column<typeof features, HistoryFile, unknown>): "ascending" | "descending" | "none" {
  const sorted = column.getIsSorted()
  if (sorted === "asc") return "ascending"
  if (sorted === "desc") return "descending"
  return "none"
}

function SortHeader({
  column,
  label,
}: {
  column: Column<typeof features, HistoryFile, unknown>
  label: string
}) {
  const sorted = column.getIsSorted()
  return (
    <button
      type="button"
      aria-label={`Sort by ${label}`}
      className="inline-flex items-center gap-1 rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
      onClick={() => column.toggleSorting()}
    >
      {label}
      {sorted === "asc" ? (
        <ArrowUp aria-hidden="true" className="size-2 text-primary" />
      ) : sorted === "desc" ? (
        <ArrowDown aria-hidden="true" className="size-2 text-primary" />
      ) : (
        <ChevronsUpDown aria-hidden="true" className="size-2 opacity-50" />
      )}
    </button>
  )
}

function StatusBadge({ status }: { status: FileStatus }) {
  const tone = HISTORY_STATUS_TONES[status] ?? HISTORY_STATUS_TONES.cancelled
  return (
    <Badge
      variant="secondary"
      className={`h-[19px] gap-[5px] rounded-full border px-[7px] text-[10px] font-semibold ${tone.tint} ${tone.border} ${tone.text}`}
    >
      <span aria-hidden="true" className={`size-[5px] rounded-full ${tone.accent}`} />
      {HISTORY_STATUS_LABELS[status] ?? status}
    </Badge>
  )
}

function RevealMenu({ file }: { file: HistoryFile }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="File actions"
            className="size-6"
          />
        }
      >
        <Ellipsis />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        <DropdownMenuItem
          disabled={file.outputPath === null}
          onClick={() => file.outputPath && void revealFile(file.outputPath)}
        >
          <FolderOpen />
          Show compressed
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => void revealFile(file.path)}>
          <Folder />
          Show original
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function sizeSortValue(file: HistoryFile): number | null {
  return currentSizeBytes(file)
}

export function HistoryFilesTable({
  session,
}: {
  session: HistorySession
}): ReactElement {
  const [search, setSearch] = useState("")
  const [type, setType] = useState<HistoryTypeFilter>("all")
  const [outcome, setOutcome] = useState<HistoryOutcomeFilter>("all")
  const [sorting, setSorting] = useState<SortingState>([])
  const [expandedPath, setExpandedPath] = useState<string | null>(null)

  const filtered = useMemo(
    () => filterHistoryFiles(session.files, { search, type, outcome }),
    [session.files, search, type, outcome]
  )
  const counts = useMemo(() => historyOutcomeCounts(session.files), [session.files])

  useEffect(() => {
    setSearch("")
    setType("all")
    setOutcome("all")
    setSorting([])
    setExpandedPath(null)
  }, [session.sessionId])

  useEffect(() => {
    if (expandedPath !== null && !filtered.some((file) => file.path === expandedPath)) {
      setExpandedPath(null)
    }
  }, [expandedPath, filtered])

  const columns = useMemo<ColumnDef<typeof features, HistoryFile>[]>(
    () => [
      {
        id: "expand",
        header: () => null,
        enableSorting: false,
        cell: ({ row }) => {
          const file = row.original
          const open = expandedPath === file.path
          const detailId = `history-detail-${row.id}`
          return (
            <button
              type="button"
              aria-label={open ? `Collapse ${file.name}` : `Expand ${file.name}`}
              aria-expanded={open}
              aria-controls={open ? detailId : undefined}
              className="flex size-[17px] items-center justify-center rounded-[5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
              onClick={() =>
                setExpandedPath((current) => (current === file.path ? null : file.path))
              }
            >
              {open ? (
                <ChevronDown aria-hidden="true" className="size-2.5" />
              ) : (
                <ChevronRight aria-hidden="true" className="size-2.5" />
              )}
            </button>
          )
        },
      },
      {
        id: "file",
        accessorFn: (row) => row.name,
        header: ({ column }) => <SortHeader column={column} label="File" />,
        cell: ({ row }) => {
          const file = row.original
          const tone = CATEGORY_TONES[file.category] ?? CATEGORY_TONES.other
          return (
            <div className="flex min-w-0 items-center gap-2">
              <span
                className={`flex size-6 shrink-0 items-center justify-center rounded-[7px] ${tone.tint} ${tone.text}`}
              >
                <CategoryIcon category={file.category} size={12} />
              </span>
              <div className="min-w-0">
                <div
                  data-testid="history-file-name"
                  className="truncate text-[12px] font-[550] text-foreground"
                  title={file.name}
                >
                  {file.name}
                </div>
                {file.error !== null && (
                  <div
                    className="truncate text-[9.5px] text-destructive"
                    title={file.error}
                  >
                    {file.error}
                  </div>
                )}
              </div>
            </div>
          )
        },
        sortFn: (a: HistoryRow, b: HistoryRow) =>
          a.original.name.localeCompare(b.original.name),
        sortDescFirst: false,
      },
      {
        id: "size",
        accessorFn: (row) => sizeSortValue(row),
        header: ({ column }) => <SortHeader column={column} label="Size" />,
        cell: ({ row }) => (
          <span className="block text-right tabular-nums">
            {currentSizeLabel(row.original)}
          </span>
        ),
        sortDescFirst: false,
        sortFn: (a: HistoryRow, b: HistoryRow) => {
          const left = sizeSortValue(a.original)
          const right = sizeSortValue(b.original)
          if (left === null && right === null) return 0
          if (left === null) return 1
          if (right === null) return -1
          return left - right
        },
      },
      {
        id: "status",
        accessorFn: (row) => HISTORY_STATUS_ORDER[row.status] ?? 99,
        header: ({ column }) => <SortHeader column={column} label="Status" />,
        cell: ({ row }) => <StatusBadge status={row.original.status} />,
        sortDescFirst: false,
        sortFn: (a: HistoryRow, b: HistoryRow) =>
          (HISTORY_STATUS_ORDER[a.original.status] ?? 99) -
          (HISTORY_STATUS_ORDER[b.original.status] ?? 99),
      },
      {
        id: "actions",
        header: () => null,
        enableSorting: false,
        cell: ({ row }) => <RevealMenu file={row.original} />,
      },
    ],
    [expandedPath]
  )

  const table = useTable({
    features,
    data: filtered,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
  })

  const clearFilters = () => {
    setSearch("")
    setType("all")
    setOutcome("all")
  }

  const rows = table.getRowModel().rows
  const scrollRef = useRef<HTMLDivElement>(null)
  const virtualize = shouldVirtualizeRows(rows.length)
  const detailRowIndex =
    expandedPath === null ? -1 : rows.findIndex((row) => row.original.path === expandedPath)
  const virtualEntry = (index: number) => {
    if (detailRowIndex >= 0 && index === detailRowIndex + 1) {
      return { kind: "detail" as const, row: rows[detailRowIndex] }
    }
    const rowIndex = detailRowIndex >= 0 && index > detailRowIndex + 1 ? index - 1 : index
    return { kind: "row" as const, row: rows[rowIndex] }
  }

  const virtualizer = useVirtualizer({
    count: rows.length + (detailRowIndex >= 0 ? 1 : 0),
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) =>
      detailRowIndex >= 0 && index === detailRowIndex + 1 ? 180 : 43,
    getItemKey: (index) => {
      const entry = virtualEntry(index)
      return virtualItemKey(entry.kind, entry.row.original.path)
    },
    overscan: 8,
    initialRect: { width: 900, height: 480 },
  })
  const virtualItems = virtualizer.getVirtualItems()
  const firstVirtual = virtualItems[0]
  const lastVirtual = virtualItems[virtualItems.length - 1]
  const topSpacer = firstVirtual?.start ?? 0
  const bottomSpacer = lastVirtual ? virtualizer.getTotalSize() - lastVirtual.end : 0

  const renderFileRow = (row: HistoryRow) => {
    const file = row.original
    const failed = file.status === "failed"
    return (
      <TableRow
        key={`row-${row.id}`}
        data-testid="history-file-row"
        className={cn(
          "h-[43px] border-b border-border/60 hover:bg-muted/40 has-aria-expanded:bg-muted/20",
          failed && "bg-destructive/5 hover:bg-destructive/10"
        )}
      >
        {row.getAllCells().map((cell) => (
          <TableCell
            key={cell.id}
            className={cn(
              "h-[43px] overflow-hidden px-[9px] py-0 whitespace-nowrap",
              cell.column.id === "actions" && "text-right"
            )}
          >
            {flexRender(cell.column.columnDef.cell, cell.getContext())}
          </TableCell>
        ))}
      </TableRow>
    )
  }

  const renderDetailRow = (
    row: HistoryRow,
    virtualIndex?: number
  ) => (
    <TableRow
      key={`detail-${row.id}`}
      id={`history-detail-${row.id}`}
      data-testid="history-detail-row"
      className="border-b border-border/60 bg-muted/10 hover:bg-muted/10"
    >
      <TableCell colSpan={5} className="p-0 whitespace-normal">
        {virtualIndex === undefined ? (
          <HistoryFileDetail
            file={row.original}
            onReveal={(path) => void revealFile(path)}
          />
        ) : (
          <div ref={virtualizer.measureElement} data-index={virtualIndex}>
            <HistoryFileDetail
              file={row.original}
              onReveal={(path) => void revealFile(path)}
            />
          </div>
        )}
      </TableCell>
    </TableRow>
  )

  return (

    <div
      data-testid="history-table-card"
      className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[16px] border border-border bg-card px-4 pt-3.5 pb-0 @max-[860px]:px-[13px] @max-[860px]:pt-3"
    >
      <div className="shrink-0">
        <HistoryTableToolbar
          search={search}
          onSearchChange={setSearch}
          type={type}
          onTypeChange={setType}
          outcome={outcome}
          onOutcomeChange={setOutcome}
          counts={counts}
          visibleCount={filtered.length}
          totalCount={session.files.length}
          onClear={clearFilters}
        />
      </div>

      <div
        ref={scrollRef}
        data-testid="history-table-scroll"
        data-virtualized={virtualize ? "true" : "false"}
        className="mt-2 min-h-0 flex-1 overflow-x-hidden overflow-y-auto border-t border-border/60"
      >
        <Table className="overflow-visible" tableClassName="table-fixed">
          <colgroup>
            <col className="w-6" />
            <col />
            <col className="w-[92px] @max-[860px]:w-[78px]" />
            <col className="w-[142px] @max-[860px]:w-[126px]" />
            <col className="w-[34px]" />
          </colgroup>
          <TableHeader>
            {table.getHeaderGroups().map((headerGroup) => (
              <TableRow key={headerGroup.id} className="hover:bg-transparent">
                {headerGroup.headers.map((header) => {
                  const label = HEADER_LABELS[header.column.id]
                  return (
                    <TableHead
                      key={header.id}
                      aria-label={label}
                      aria-sort={label ? ariaSort(header.column) : undefined}
                      style={{
                        backgroundColor:
                          "color-mix(in oklch, var(--card) 94%, var(--foreground) 6%)",
                      }}
                      className={cn(
                        "sticky top-0 z-10 h-8 px-[9px] text-[9.5px] font-semibold tracking-[0.06em] text-muted-foreground uppercase",
                        header.column.id === "size" && "text-right",
                        header.column.id === "actions" && "text-right"
                      )}
                    >
                      {header.isPlaceholder
                        ? null
                        : flexRender(header.column.columnDef.header, header.getContext())}
                    </TableHead>
                  )
                })}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {virtualize ? (
              <>
                {topSpacer > 0 && (
                  <tr aria-hidden="true" className="border-0">
                    <td colSpan={5} className="p-0" style={{ height: topSpacer }} />
                  </tr>
                )}
                {virtualItems.map((item) => {
                  const entry = virtualEntry(item.index)
                  if (entry.row === undefined) return null
                  return entry.kind === "detail" ? (
                    renderDetailRow(entry.row, item.index)
                  ) : (
                    renderFileRow(entry.row)
                  )
                })}
                {bottomSpacer > 0 && (
                  <tr aria-hidden="true" className="border-0">
                    <td colSpan={5} className="p-0" style={{ height: bottomSpacer }} />
                  </tr>
                )}
              </>
            ) : rows.length === 0 ? (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={5} className="h-[120px] p-0 text-center whitespace-normal">
                  <div className="flex flex-col items-center justify-center gap-3">
                    <p className="text-[12px] text-muted-foreground">
                      No files match these filters.
                    </p>
                    <Button variant="outline" size="sm" onClick={clearFilters}>
                      Clear filters
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ) : (
              rows.map((row) => (
                <Fragment key={row.id}>
                  {renderFileRow(row)}
                  {expandedPath === row.original.path && renderDetailRow(row)}
                </Fragment>
              ))
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}
