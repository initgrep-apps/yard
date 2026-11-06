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

import { useEffect, useRef, useState } from "react"
import { startOfDay } from "date-fns"
import { Calendar as CalendarIcon, ChevronDown, X } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Calendar } from "@/components/ui/calendar"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { FieldLegend, FieldSet } from "@/components/ui/field"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { CategoryIcon } from "@/components/CategoryIcon"
import {
  DATE_PRESET_LABELS,
  DATE_PRESETS,
  describeSince,
  presetSinceMs,
} from "@/lib/historyFormat"
import { HISTORY_STATUS_TONES } from "@/lib/historyTable"
import { CATEGORY_TONES, STATUS_TONES } from "@/lib/tokens"
import { cn } from "@/lib/utils"
import {
  CATEGORIES,
  type Category,
  type FileStatus,
  type HistoryFilters,
} from "@/types"

const CHIP_CLASS =
  "rounded-full px-3 data-[pressed]:border-primary/50 data-[pressed]:bg-primary/15 data-[pressed]:text-foreground"

const EMPTY_DRAFT: HistoryFilters = Object.freeze({
  search: undefined,
  outputFolder: undefined,
  statuses: undefined,
  categories: undefined,
  sinceMs: undefined,
  sort: "newest",
})

type FilterableStatus = Exclude<FileStatus, "pending" | "compressing">

const STATUS_LABELS: Record<FilterableStatus, string> = {
  done: "Done",
  best_effort: "Best effort",
  failed: "Failed",
  cancelled: "Cancelled",
  already_under_target: "Under target",
  needs_permission: "Needs permission",
  unsupported: "Unsupported",
}

const isFilterableStatus = (status: FileStatus): status is FilterableStatus =>
  Object.prototype.hasOwnProperty.call(STATUS_LABELS, status)

function visibleFilterCount(filters: HistoryFilters): number {
  return (
    (filters.statuses?.length ?? 0) +
    (filters.categories?.length ?? 0) +
    (filters.sinceMs !== undefined ? 1 : 0) +
    (filters.sort === "oldest" ? 1 : 0)
  )
}

const STATUS_CHOICES = Object.entries(STATUS_LABELS).map(([value, label]) => ({
  value: value as FilterableStatus,
  label,
}))

const CATEGORY_LABELS: Record<Category, string> = {
  image: "Image",
  video: "Video",
  pdf: "PDF",
  document: "Document",
  other: "Other",
}

const CATEGORY_CHOICES = CATEGORIES.map((value) => ({
  value,
  label: CATEGORY_LABELS[value],
}))

interface HistoryFilterDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  applied: HistoryFilters
  onApply: (filters: HistoryFilters) => void
}

export function HistoryFilterDialog({
  open,
  onOpenChange,
  applied,
  onApply,
}: HistoryFilterDialogProps) {
  const [draft, setDraft] = useState<HistoryFilters>(EMPTY_DRAFT)
  const [sinceOpen, setSinceOpen] = useState(false)
  const wasOpen = useRef(false)

  useEffect(() => {
    if (open && !wasOpen.current) {
      const statuses = applied.statuses?.filter(isFilterableStatus)
      const categories = applied.categories?.filter((category) =>
        CATEGORIES.includes(category),
      )
      const seed: HistoryFilters = {
        ...EMPTY_DRAFT,
        ...applied,
        statuses: statuses?.length ? statuses : undefined,
        categories: categories?.length ? categories : undefined,
        sort: applied.sort === "oldest" ? "oldest" : "newest",
        sinceMs: Number.isFinite(applied.sinceMs) ? applied.sinceMs : undefined,
      }
      setDraft(seed)
      setSinceOpen(false)
    }
    wasOpen.current = open
  }, [open, applied])

  const update = (patch: Partial<HistoryFilters>) =>
    setDraft((current) => ({ ...current, ...patch }))

  const now = new Date()
  const sortValue = draft.sort === "oldest" ? "oldest" : "newest"
  const activeCount = visibleFilterCount(draft)
  const selectedDate =
    draft.sinceMs === undefined ? undefined : new Date(draft.sinceMs)

  const apply = () => {
    onApply({
      ...applied,
      statuses: draft.statuses,
      categories: draft.categories,
      sinceMs: draft.sinceMs,
      sort: draft.sort,
    })
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={false}
        className="flex max-h-[90%] flex-col gap-0 overflow-hidden p-0 sm:max-w-[600px]"
      >
        <DialogHeader className="flex h-[54px] shrink-0 flex-row items-center gap-2.5 border-b border-border px-5">
          <DialogTitle>Filter history</DialogTitle>
          {activeCount > 0 && (
            <Badge
              variant="secondary"
              className={cn(
                "border",
                STATUS_TONES.done.tint,
                STATUS_TONES.done.border,
                STATUS_TONES.done.text,
              )}
            >
              {activeCount} active
            </Badge>
          )}
          <DialogClose
            render={
              <Button
                variant="ghost"
                size="icon-sm"
                className="ml-auto"
                aria-label="Close"
                title="Close"
              />
            }
          >
            <X />
          </DialogClose>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-5">
          <FieldSet className="min-w-0 gap-2.5 border-b border-border py-3">
            <FieldLegend className="mb-0 data-[variant=legend]:text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Sort by
            </FieldLegend>
            <ToggleGroup
              aria-label="Sort by"
              value={[sortValue]}
              onValueChange={(value) => update({ sort: value[0] ?? sortValue })}
              className="w-full rounded-xl border border-border bg-muted/40 p-0.5"
            >
              <ToggleGroupItem value="newest" size="sm" className="flex-1">
                Newest first
              </ToggleGroupItem>
              <ToggleGroupItem value="oldest" size="sm" className="flex-1">
                Oldest first
              </ToggleGroupItem>
            </ToggleGroup>
          </FieldSet>

          <FieldSet className="min-w-0 gap-2.5 border-b border-border py-3">
            <FieldLegend className="mb-0 data-[variant=legend]:text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Status
            </FieldLegend>
            <ToggleGroup
              aria-label="Status"
              multiple
              value={draft.statuses ?? []}
              onValueChange={(value) =>
                update({ statuses: value.length > 0 ? value : undefined })
              }
              className="flex w-full flex-wrap gap-1.5"
            >
              {STATUS_CHOICES.map(({ value, label }) => (
                <ToggleGroupItem
                  key={value}
                  value={value}
                  variant="outline"
                  size="sm"
                  className={CHIP_CLASS}
                >
                  <span
                    aria-hidden="true"
                    className={cn(
                      "size-1.5 shrink-0 rounded-full",
                      HISTORY_STATUS_TONES[value].accent,
                    )}
                  />
                  {label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </FieldSet>

          <FieldSet className="min-w-0 gap-2.5 border-b border-border py-3">
            <FieldLegend className="mb-0 data-[variant=legend]:text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Category
            </FieldLegend>
            <ToggleGroup
              aria-label="Category"
              multiple
              value={draft.categories ?? []}
              onValueChange={(value) =>
                update({ categories: value.length > 0 ? value : undefined })
              }
              className="flex w-full flex-wrap gap-1.5"
            >
              {CATEGORY_CHOICES.map(({ value, label }) => (
                <ToggleGroupItem
                  key={value}
                  value={value}
                  variant="outline"
                  size="sm"
                  className={CHIP_CLASS}
                >
                  <CategoryIcon
                    category={value}
                    className={CATEGORY_TONES[value].text}
                  />
                  {label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </FieldSet>

          <FieldSet className="min-w-0 gap-2.5 py-3">
            <FieldLegend className="mb-0 data-[variant=legend]:text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Since
            </FieldLegend>
            <Popover open={sinceOpen} onOpenChange={setSinceOpen}>
              <PopoverTrigger
                render={
                  <Button
                    variant="outline"
                    className="w-full justify-start gap-2 font-normal"
                  />
                }
              >
                <CalendarIcon aria-hidden="true" />
                {describeSince(draft.sinceMs, now)}
                <ChevronDown className="ml-auto" aria-hidden="true" />
              </PopoverTrigger>
              <PopoverContent
                align="start"
                className="w-auto flex-row gap-0 p-0"
              >
                <div className="flex w-32 flex-col gap-0.5 border-r border-border p-1.5">
                  {DATE_PRESETS.map((preset) => {
                    const active =
                      DATE_PRESET_LABELS[preset] ===
                      describeSince(draft.sinceMs, now)
                    return (
                      <Button
                        key={preset}
                        variant={active ? "secondary" : "ghost"}
                        size="sm"
                        aria-pressed={active}
                        className="justify-start"
                        onClick={() => {
                          update({
                            sinceMs: presetSinceMs(preset, new Date()),
                          })
                          setSinceOpen(false)
                        }}
                      >
                        {DATE_PRESET_LABELS[preset]}
                      </Button>
                    )
                  })}
                </div>
                <Calendar
                  mode="single"
                  selected={selectedDate}
                  defaultMonth={selectedDate}
                  disabled={{ after: new Date() }}
                  onSelect={(date) => {
                    update({
                      sinceMs: date ? startOfDay(date).getTime() : undefined,
                    })
                    setSinceOpen(false)
                  }}
                />
              </PopoverContent>
            </Popover>
          </FieldSet>
        </div>

        <div className="flex shrink-0 items-center gap-2 border-t border-border px-5 py-3">
          <Button variant="ghost" onClick={() => setDraft(EMPTY_DRAFT)}>
            Clear all
          </Button>
          <span className="flex-1" />
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={apply}>Apply</Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
