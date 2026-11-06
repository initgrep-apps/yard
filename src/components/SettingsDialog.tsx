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

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react"
import { Clock, FolderOpen, Settings as SettingsIcon, ShieldAlert, ShieldCheck, SlidersHorizontal, Zap } from "lucide-react"
import { toast } from "sonner"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Toggle } from "@/components/ui/toggle"
import { NumberStepper } from "@/components/NumberStepper"
import { useMemory } from "@/hooks/useMemory"
import { useSettings } from "@/hooks/useSettings"
import { formatSize } from "@/lib/sizeFormat"
import { useHistoryContext } from "@/providers/HistoryProvider"
import type {
  Category,
  CategoryTarget,
  ConsentSettings,
  PerformanceSettings,
  QualityFloors,
  Settings,
} from "@/types"

const SAVE_DEBOUNCE_MS = 400
const MAX_FLUSH_WRITES = 3
const SAVE_ERROR = "Could not save settings"
const RESET_ERROR = "Could not reset settings"
const PENDING_ERROR = "Settings still not saved — close again to retry"
const SAVE_ERROR_ID = "settings-save"
const RESET_ERROR_ID = "settings-reset"

type PersistResult = "saved" | "failed" | "pending"

const SECTIONS = [
  { value: "general", label: "General", icon: SettingsIcon },
  { value: "targets", label: "Targets", icon: SlidersHorizontal },
  { value: "floors", label: "Quality floors", icon: ShieldCheck },
  { value: "consent", label: "Consent", icon: ShieldAlert },
  { value: "performance", label: "Performance", icon: Zap },
  { value: "history", label: "History", icon: Clock },
] as const

type SectionId = (typeof SECTIONS)[number]["value"]

const CATEGORIES = [
  { key: "image", label: "Images", formats: "JPEG, PNG, WebP, HEIC" },
  { key: "video", label: "Videos", formats: "MP4, MOV" },
  { key: "pdf", label: "PDF", formats: "PDF documents" },
  { key: "document", label: "Documents", formats: "DOCX, XLSX, PPTX" },
] as const

function targetFor(settings: Settings, key: Category): CategoryTarget {
  return settings.categoryTargets[key] ?? { size: 0, unit: "KB" }
}

interface SettingsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

interface SectionProps {
  draft: Settings
  onChange: (next: Settings) => void
}

function GeneralSection({
  draft,
  onChoose,
}: Pick<SectionProps, "draft"> & { onChoose: () => void }) {
  return (
    <FieldSet>
      <FieldLegend>General</FieldLegend>
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="output-folder">Output folder</FieldLabel>
          <FieldDescription>
            Compressed copies are written here. Originals are never modified.
          </FieldDescription>
          <div className="flex items-center gap-2">
            <Input
              id="output-folder"
              readOnly
              value={draft.outputFolder}
              className="flex-1"
            />
            <Button
              variant="outline"
              size="icon"
              aria-label="Choose output folder"
              title="Choose folder"
              onClick={onChoose}
            >
              <FolderOpen />
            </Button>
          </div>
        </Field>
      </FieldGroup>
    </FieldSet>
  )
}

function TargetsSection({ draft, onChange }: SectionProps) {
  const setTarget = (key: Category, patch: Partial<CategoryTarget>) => {
    const current = targetFor(draft, key)
    onChange({
      ...draft,
      categoryTargets: {
        ...draft.categoryTargets,
        [key]: { ...current, ...patch },
      },
    })
  }

  return (
    <FieldSet>
      <FieldLegend>Targets</FieldLegend>
      <FieldGroup>
        {CATEGORIES.map((category) => {
          const target = targetFor(draft, category.key)
          return (
            <Field key={category.key} orientation="horizontal">
              <FieldContent>
                <FieldLabel htmlFor={`${category.key}-size`}>
                  {category.label}
                </FieldLabel>
                <FieldDescription>{category.formats}</FieldDescription>
              </FieldContent>
              <div className="flex shrink-0 items-center gap-1.5">
                <NumberStepper
                  id={`${category.key}-size`}
                  value={target.size}
                  onValueChange={(size) => setTarget(category.key, { size })}
                  min={0}
                  ariaLabel={`${category.label} target size`}
                  className="w-32"
                />
                <Select
                  value={target.unit}
                  onValueChange={(unit) => {
                    if (unit !== null) setTarget(category.key, { unit })
                  }}
                >
                  <SelectTrigger
                    aria-label={`${category.label} target unit`}
                    className="w-20"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="KB">KB</SelectItem>
                    <SelectItem value="MB">MB</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </Field>
          )
        })}
      </FieldGroup>
    </FieldSet>
  )
}

function FloorsSection({ draft, onChange }: SectionProps) {
  const setFloors = (floors: QualityFloors) =>
    onChange({ ...draft, qualityFloors: floors })
  const floors = draft.qualityFloors

  return (
    <FieldSet>
      <FieldLegend>Quality floors</FieldLegend>
      <FieldDescription>
        The engine stops compressing below these values and reports best effort.
      </FieldDescription>
      <FieldGroup>
        <Field orientation="horizontal">
          <FieldLabel htmlFor="image-min-quality">
            Images minimum quality
          </FieldLabel>
          <div className="flex shrink-0 items-center gap-1.5">
            <NumberStepper
              id="image-min-quality"
              value={floors.image.minQuality}
              onValueChange={(minQuality) =>
                setFloors({
                  ...floors,
                  image: { ...floors.image, minQuality },
                })
              }
              min={0}
              max={100}
              ariaLabel="Images minimum quality"
              className="w-32"
            />
            <span className="w-4 text-xs text-muted-foreground">%</span>
          </div>
        </Field>
        <Field orientation="horizontal">
          <FieldLabel htmlFor="image-min-long-edge">
            Images minimum long edge
          </FieldLabel>
          <div className="flex shrink-0 items-center gap-1.5">
            <NumberStepper
              id="image-min-long-edge"
              value={floors.image.minLongEdge}
              onValueChange={(minLongEdge) =>
                setFloors({
                  ...floors,
                  image: { ...floors.image, minLongEdge },
                })
              }
              min={1}
              max={10000}
              ariaLabel="Images minimum long edge"
              className="w-32"
            />
            <span className="w-6 text-xs text-muted-foreground">px</span>
          </div>
        </Field>
        <Field orientation="horizontal">
          <FieldLabel htmlFor="video-min-crf">Videos minimum CRF</FieldLabel>
          <NumberStepper
            id="video-min-crf"
            value={floors.video.minCrf}
            onValueChange={(minCrf) =>
              setFloors({ ...floors, video: { ...floors.video, minCrf } })
            }
            min={0}
            max={51}
            ariaLabel="Videos minimum CRF"
            className="w-32"
          />
        </Field>
        <Field orientation="horizontal">
          <FieldLabel>Videos minimum resolution</FieldLabel>
          <Select
            value={floors.video.minResolution}
            onValueChange={(minResolution) => {
              if (minResolution !== null) {
                setFloors({
                  ...floors,
                  video: { ...floors.video, minResolution },
                })
              }
            }}
          >
            <SelectTrigger
              aria-label="Videos minimum resolution"
              className="w-24"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="480p">480p</SelectItem>
              <SelectItem value="720p">720p</SelectItem>
              <SelectItem value="1080p">1080p</SelectItem>
            </SelectContent>
          </Select>
        </Field>
        <Field orientation="horizontal">
          <FieldLabel htmlFor="pdf-min-dpi">PDF minimum DPI</FieldLabel>
          <div className="flex shrink-0 items-center gap-1.5">
            <NumberStepper
              id="pdf-min-dpi"
              value={floors.pdf.minDpi}
              onValueChange={(minDpi) =>
                setFloors({ ...floors, pdf: { ...floors.pdf, minDpi } })
              }
              min={0}
              max={600}
              ariaLabel="PDF minimum DPI"
              className="w-32"
            />
            <span className="w-6 text-xs text-muted-foreground">dpi</span>
          </div>
        </Field>
        <Field orientation="horizontal">
          <FieldLabel htmlFor="pdf-min-jpeg-quality">
            PDF minimum JPEG quality
          </FieldLabel>
          <div className="flex shrink-0 items-center gap-1.5">
            <NumberStepper
              id="pdf-min-jpeg-quality"
              value={floors.pdf.minJpegQuality}
              onValueChange={(minJpegQuality) =>
                setFloors({
                  ...floors,
                  pdf: { ...floors.pdf, minJpegQuality },
                })
              }
              min={0}
              max={100}
              ariaLabel="PDF minimum JPEG quality"
              className="w-32"
            />
            <span className="w-4 text-xs text-muted-foreground">%</span>
          </div>
        </Field>
        <Field orientation="horizontal">
          <FieldLabel htmlFor="document-min-media-quality">
            Documents media quality
          </FieldLabel>
          <div className="flex shrink-0 items-center gap-1.5">
            <NumberStepper
              id="document-min-media-quality"
              value={floors.document.minMediaQuality}
              onValueChange={(minMediaQuality) =>
                setFloors({
                  ...floors,
                  document: { minMediaQuality },
                })
              }
              min={0}
              max={100}
              ariaLabel="Documents media quality"
              className="w-32"
            />
            <span className="w-4 text-xs text-muted-foreground">%</span>
          </div>
        </Field>
      </FieldGroup>
    </FieldSet>
  )
}

function ConsentSection({ draft, onChange }: SectionProps) {
  const setConsent = (patch: Partial<ConsentSettings>) =>
    onChange({ ...draft, consent: { ...draft.consent, ...patch } })

  return (
    <FieldSet>
      <FieldLegend>Consent</FieldLegend>
      <FieldDescription>
        These grants apply to every batch. Anything not granted here is asked
        for in the batch pre-flight.
      </FieldDescription>
      <FieldGroup>
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel>Allow destructive changes</FieldLabel>
            <FieldDescription>
              Fewer pixels, dropped streams or pages, format changes, and
              rasterisation.
            </FieldDescription>
          </FieldContent>
          <Toggle
            variant="outline"
            pressed={draft.consent.allowDestructive}
            onPressedChange={(allowDestructive) =>
              setConsent({ allowDestructive })
            }
            aria-label="Allow destructive changes"
          >
            {draft.consent.allowDestructive ? "On" : "Off"}
          </Toggle>
        </Field>
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel>Convert HEIC/HEIF to JPEG</FieldLabel>
            <FieldDescription>
              On by default. Turn it off and the batch pre-flight asks before
              converting.
            </FieldDescription>
          </FieldContent>
          <Toggle
            variant="outline"
            pressed={draft.consent.heicToJpg}
            onPressedChange={(heicToJpg) => setConsent({ heicToJpg })}
            aria-label="Convert HEIC/HEIF to JPEG"
          >
            {draft.consent.heicToJpg ? "On" : "Off"}
          </Toggle>
        </Field>
      </FieldGroup>
    </FieldSet>
  )
}

function PerformanceSection({ draft, onChange }: SectionProps) {
  const { stats, batchUsage } = useMemory()
  const setPerformance = (patch: Partial<PerformanceSettings>) =>
    onChange({
      ...draft,
      performance: { ...draft.performance, ...patch },
    })

  return (
    <FieldSet>
      <FieldLegend>Performance</FieldLegend>
      <FieldDescription>
        Memory budget controls how many files compress at once.
      </FieldDescription>
      <FieldGroup>
        <Field orientation="horizontal">
          <FieldLabel>Memory mode</FieldLabel>
          <Select
            value={draft.performance.adaptiveMemory ? 'adaptive' : 'manual'}
            onValueChange={(mode) => {
              if (mode !== null) {
                setPerformance({ adaptiveMemory: mode === 'adaptive' })
              }
            }}
          >
            <SelectTrigger aria-label="Memory mode" className="w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="adaptive">Adaptive</SelectItem>
              <SelectItem value="manual">Manual</SelectItem>
            </SelectContent>
          </Select>
        </Field>
        <Field orientation="horizontal">
          <FieldLabel htmlFor="memory-ceiling">Memory ceiling</FieldLabel>
          <NumberStepper
            id="memory-ceiling"
            value={draft.performance.memoryCeilingMb}
            onValueChange={(memoryCeilingMb) =>
              setPerformance({ memoryCeilingMb })
            }
            min={300}
            max={2048}
            ariaLabel="Memory ceiling in megabytes"
            className="w-32"
          />
        </Field>
      </FieldGroup>
      {stats && (
        <div className={`mt-4 grid gap-2 text-sm ${batchUsage ? 'grid-cols-4' : 'grid-cols-3'}`}>
          <div>
            <div className="text-muted-foreground">Total RAM</div>
            <div className="font-medium">{formatSize(stats.totalRamBytes)}</div>
          </div>
          <div>
            <div className="text-muted-foreground">Budget</div>
            <div className="font-medium">
              {formatSize(batchUsage?.budgetBytes ?? stats.budgetBytes)}
            </div>
          </div>
          <div>
            <div className="text-muted-foreground">App RSS</div>
            <div className="font-medium">
              {formatSize(batchUsage?.processRssBytes ?? stats.processRssBytes)}
            </div>
          </div>
          {batchUsage && (
            <div>
              <div className="text-muted-foreground">Batch used</div>
              <div className="font-medium">{formatSize(batchUsage.usedBytes)}</div>
            </div>
          )}
        </div>
      )}
    </FieldSet>
  )
}

function HistorySection({ draft, onChange, clearHistory }: SectionProps & { clearHistory: () => Promise<void> }) {
  return (
    <FieldSet>
      <FieldLegend>History</FieldLegend>
      <FieldDescription>
        Sessions older than this are removed automatically. Files on disk are
        never deleted.
      </FieldDescription>
      <FieldGroup>
        <Field orientation="horizontal">
          <FieldLabel htmlFor="history-retention-days">
            History retention (days)
          </FieldLabel>
          <NumberStepper
            id="history-retention-days"
            value={draft.history.retentionDays}
            onValueChange={(retentionDays) =>
              onChange({ ...draft, history: { retentionDays } })
            }
            min={1}
            max={3650}
            ariaLabel="History retention (days)"
            className="w-32"
          />
        </Field>
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel>Clear history</FieldLabel>
            <FieldDescription>Permanently deletes all compression history. Files on disk are not affected.</FieldDescription>
          </FieldContent>
          <AlertDialog>
            <AlertDialogTrigger render={<Button variant="destructive" size="sm" />}>Clear history</AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Clear history?</AlertDialogTitle>
                <AlertDialogDescription>
                  Permanently deletes all compression history. Files on disk are not affected.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction variant="destructive" onClick={() => void clearHistory()}>
                  Delete history
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </Field>
      </FieldGroup>
    </FieldSet>
  )
}

export function SettingsDialog({ open, onOpenChange }: SettingsDialogProps) {
  const {
    settings,
    loading,
    error,
    getLastError,
    saveSettings,
    resetSettings,
    browseFolder,
  } = useSettings()
  const { clearHistory } = useHistoryContext()
  const [draft, setDraft] = useState<Settings | null>(null)
  const [section, setSection] = useState<SectionId>("general")
  const [flushVersion, setFlushVersion] = useState(0)
  const draftRef = useRef<Settings | null>(null)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const inFlightRef = useRef<Promise<boolean> | null>(null)
  const dirtyRef = useRef(false)
  const seededRef = useRef(false)
  const reopenedWhileWritingRef = useRef(false)

  const clearTimer = () => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }

  const armDebounce = () => {
    clearTimer()
    timerRef.current = setTimeout(() => {
      timerRef.current = null
      void persist()
    }, SAVE_DEBOUNCE_MS)
  }

  const persist = useCallback(async (): Promise<PersistResult> => {
    clearTimer()
    for (let writes = 0; writes < MAX_FLUSH_WRITES; writes++) {
      while (inFlightRef.current) await inFlightRef.current
      if (!dirtyRef.current || !draftRef.current) return "saved"

      dirtyRef.current = false
      const request = saveSettings(draftRef.current)
      inFlightRef.current = request
      const saved = await request
      inFlightRef.current = null
      setFlushVersion((version) => version + 1)

      if (!saved) {
        dirtyRef.current = true
        toast.error(SAVE_ERROR, {
          id: SAVE_ERROR_ID,
          description: getLastError() ?? undefined,
          duration: Infinity,
        })
        return "failed"
      }
      toast.dismiss(SAVE_ERROR_ID)
    }
    return dirtyRef.current ? "pending" : "saved"
  }, [getLastError, saveSettings])

  useEffect(() => {
    if (open) return
    seededRef.current = false
    reopenedWhileWritingRef.current = false
    setDraft(null)
    if (dirtyRef.current) void persist()
  }, [open, persist])

  useEffect(() => {
    if (!open || seededRef.current) return
    if (inFlightRef.current) {
      reopenedWhileWritingRef.current = true
      return
    }
    if (reopenedWhileWritingRef.current) {
      reopenedWhileWritingRef.current = false
      if (dirtyRef.current && draftRef.current) {
        seededRef.current = true
        setDraft(draftRef.current)
        armDebounce()
        return
      }
    }
    if (!settings) return
    seededRef.current = true
    clearTimer()
    dirtyRef.current = false
    draftRef.current = settings
    setDraft(settings)
  }, [open, settings, flushVersion])

  useEffect(
    () => () => {
      if (dirtyRef.current) void persist()
    },
    [persist],
  )

  const commit = (next: Settings) => {
    draftRef.current = next
    setDraft(next)
    dirtyRef.current = true
    armDebounce()
  }

  const requestClose = async () => {
    const result = await persist()
    if (result === "saved") {
      onOpenChange(false)
      return
    }
    if (result === "pending") {
      toast.error(PENDING_ERROR, {
        id: SAVE_ERROR_ID,
        description: getLastError() ?? undefined,
        duration: Infinity,
      })
      dirtyRef.current = true
      armDebounce()
    }
  }

  const handleOpenChange = (next: boolean) => {
    if (next) {
      onOpenChange(true)
      return
    }
    void requestClose()
  }

  const handleChoose = async () => {
    const folder = await browseFolder()
    if (folder && draftRef.current) {
      commit({ ...draftRef.current, outputFolder: folder })
    }
  }

  const handleReset = async () => {
    while (inFlightRef.current) await inFlightRef.current
    const wasDirty = dirtyRef.current
    const startDraft = draftRef.current
    clearTimer()
    dirtyRef.current = false

    const request = (async (): Promise<boolean> => {
      const defaults = await resetSettings()
      setFlushVersion((version) => version + 1)
      if (!defaults) {
        toast.error(RESET_ERROR, {
          id: RESET_ERROR_ID,
          description: getLastError() ?? undefined,
          duration: Infinity,
        })
        return false
      }
      if (draftRef.current !== startDraft && draftRef.current) {
        dirtyRef.current = true
        armDebounce()
        return true
      }
      toast.dismiss(SAVE_ERROR_ID)
      toast.dismiss(RESET_ERROR_ID)
      clearTimer()
      dirtyRef.current = false
      draftRef.current = defaults
      setDraft(defaults)
      return true
    })()
    inFlightRef.current = request
    const reset = await request
    inFlightRef.current = null
    if (!reset && wasDirty) {
      dirtyRef.current = true
      armDebounce()
    }
  }

  const panelsFor = (current: Settings, clearHistory: () => Promise<void>): Record<SectionId, ReactNode> => ({
    general: <GeneralSection draft={current} onChoose={handleChoose} />,
    targets: <TargetsSection draft={current} onChange={commit} />,
    floors: <FloorsSection draft={current} onChange={commit} />,
    consent: <ConsentSection draft={current} onChange={commit} />,
    performance: <PerformanceSection draft={current} onChange={commit} />,
    history: <HistorySection draft={current} onChange={commit} clearHistory={clearHistory} />,
  })

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="h-[540px] max-h-[90%] grid-rows-1 gap-0 overflow-hidden p-0 sm:max-w-[720px]">
        <DialogHeader className="sr-only">
          <DialogTitle>Settings</DialogTitle>
        </DialogHeader>
        {loading || !draft ? (
          <div className="p-6 text-sm text-muted-foreground">
            {error ?? "Loading settings…"}
          </div>
        ) : (
          <Tabs
            value={section}
            onValueChange={setSection}
            orientation="vertical"
            className="h-full min-h-0 gap-0"
          >
            <div className="w-[184px] shrink-0 border-r border-border p-2">
              <TabsList
                variant="line"
                aria-label="Settings sections"
                className="h-fit w-full flex-col items-stretch gap-0.5 bg-transparent p-0"
              >
                {SECTIONS.map(({ value, label, icon: Icon }) => (
                  <TabsTrigger
                    key={value}
                    value={value}
                    className="justify-start gap-2 px-3"
                  >
                    <Icon data-icon className="size-4" aria-hidden="true" />
                    {label}
                  </TabsTrigger>
                ))}
              </TabsList>
            </div>
            <div className="flex min-h-0 min-w-0 flex-1 flex-col">
              <ScrollArea className="min-h-0 flex-1">
                {SECTIONS.map(({ value }) => (
                  <TabsContent key={value} value={value} className="p-6">
                    {panelsFor(draft, clearHistory)[value]}
                  </TabsContent>
                ))}
              </ScrollArea>
              <div className="flex shrink-0 justify-end gap-2 border-t border-border p-4">
                <Button variant="ghost" onClick={handleReset}>
                  Reset defaults
                </Button>
                <Button onClick={() => handleOpenChange(false)}>Done</Button>
              </div>
            </div>
          </Tabs>
        )}
      </DialogContent>
    </Dialog>
  )
}
