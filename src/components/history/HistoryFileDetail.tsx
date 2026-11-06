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

import type { ReactElement } from "react"
import { Folder, FolderOpen } from "lucide-react"
import type { Change, ConsentSource, HistoryFile } from "@/types"
import { savedLabel } from "@/lib/historyFormat"
import {
  completedLabel,
  fileSizeSummary,
  formatPreciseDuration,
  historyMethodLabel,
  isNoOutputFile,
} from "@/lib/historyTable"
import { formatSize } from "@/lib/sizeFormat"
import { Button } from "@/components/ui/button"

const CARD_CLASS =
  "flex min-w-0 flex-col gap-2 rounded-[12px] border border-border/60 bg-muted/30 p-[11px_12px]"
const EYEBROW_CLASS =
  "text-[8px] uppercase tracking-[0.08em] text-muted-foreground"
const LABEL_CLASS = "text-[8px] leading-none text-muted-foreground"
const VALUE_CLASS = "text-[10px] leading-tight text-foreground"

const CHANGE_LABELS: Record<Change, string> = {
  lossy_encode: "Lossy encode",
  downscale: "Downscale",
  drop_stream: "Drop stream",
  format_change: "Format change",
  metadata_strip: "Metadata strip",
  rasterize: "Rasterize",
  page_drop: "Page drop",
}

const CONSENT_LABELS: Record<ConsentSource, string> = {
  default: "Default",
  global: "Global setting",
  category: "Category setting",
  batch: "This batch",
  once: "Once",
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-px">
      <span className={LABEL_CLASS}>{label}</span>
      <b className={`${VALUE_CLASS} font-medium`}>{value}</b>
    </div>
  )
}

function SizeCard({ file }: { file: HistoryFile }) {
  const summary = fileSizeSummary(file)
  const currentPercent =
    summary.originalBytes > 0 ? Math.min(100, Math.max(0, 100 - summary.percent)) : 0
  const gonePercent = summary.originalBytes > 0 ? 100 - currentPercent : 0
  const showCurrentLabel = currentPercent > 0 && currentPercent < 100

  return (
    <div className={CARD_CLASS} data-testid="history-detail-size-card">
      <div className={EYEBROW_CLASS}>Size</div>
      <div className="relative h-3 text-[8px] text-muted-foreground tabular-nums">
        <span className="absolute left-0">0</span>
        {showCurrentLabel && (
          <span
            className="absolute -translate-x-1/2 font-semibold text-foreground"
            style={{ left: `${currentPercent}%` }}
          >
            {formatSize(summary.currentBytes)}
          </span>
        )}
        <span className="absolute right-0 font-semibold text-foreground">
          {formatSize(summary.originalBytes)}
        </span>
      </div>
      <div
        role="img"
        aria-label={`Current size ${formatSize(summary.currentBytes)} of ${formatSize(
          summary.originalBytes
        )} original; ${summary.percent}% saved`}
        className="relative flex h-[7px] rounded-full bg-muted"
        data-testid="detail-scale"
      >
        <span
          aria-hidden="true"
          className="h-full rounded-l-full bg-primary"
          style={{ width: `${currentPercent}%` }}
          data-testid="detail-current-bar"
        />
        <span
          aria-hidden="true"
          className="h-full rounded-r-full bg-muted-foreground/20"
          style={{ width: `${gonePercent}%` }}
        />
        {currentPercent > 0 && currentPercent < 100 && (
          <span
            aria-hidden="true"
            className="absolute -top-[3px] -bottom-[3px] w-0.5 rounded-full bg-foreground"
            style={{ left: `${currentPercent}%` }}
          />
        )}
      </div>
      <div className="flex items-baseline justify-between gap-2 text-[9px] text-muted-foreground">
        <span className="font-[650] text-primary">{summary.percent}% saved</span>
        <span>{savedLabel(summary.savedBytes)} saved</span>
      </div>
    </div>
  )
}

function ErrorCard({ file }: { file: HistoryFile }) {
  return (
    <div className={CARD_CLASS} data-testid="history-detail-error-card">
      <div className={EYEBROW_CLASS}>Size</div>
      <p className="whitespace-normal break-words text-[10px] text-destructive">
        {file.error ?? "No output file."}
      </p>
    </div>
  )
}

export function HistoryFileDetail({
  file,
  onReveal,
}: {
  file: HistoryFile
  onReveal: (path: string) => void
}): ReactElement {
  return (
    <div
      className="grid grid-cols-[1.4fr_1fr_1fr] gap-3 p-[13px_16px_13px_40px] @max-[860px]:gap-2.5 @max-[860px]:pl-[30px]"
      data-testid="history-file-detail"
    >
      {isNoOutputFile(file) ? <ErrorCard file={file} /> : <SizeCard file={file} />}

      <div className={CARD_CLASS}>
        <div className={EYEBROW_CLASS}>Target &amp; method</div>
        <Fact label="Target" value={formatSize(file.targetBytes)} />
        <Fact label="Method" value={historyMethodLabel(file)} />
      </div>

      <div className={CARD_CLASS}>
        <div className={EYEBROW_CLASS}>Timing</div>
        <Fact label="Duration" value={formatPreciseDuration(file.durationMs)} />
        <Fact label="Completed" value={completedLabel(file.completedAt)} />
      </div>

      {(file.changes.length > 0 || file.consentSource !== null) && (
        <div className={CARD_CLASS} data-testid="history-detail-permission-card">
          <div className={EYEBROW_CLASS}>Permissions</div>
          {file.changes.length > 0 && (
            <Fact
              label="Changes"
              value={file.changes.map((change) => CHANGE_LABELS[change] ?? change).join(", ")}
            />
          )}
          {file.consentSource !== null && (
            <Fact
              label="Consent"
              value={CONSENT_LABELS[file.consentSource] ?? file.consentSource}
            />
          )}
        </div>
      )}

      <div className="col-span-full flex justify-end gap-1.5 border-t border-border/60 pt-[9px]">
        <Button
          variant="outline"
          size="xs"
          className="h-6 rounded-[6px] px-2 text-[10px]"
          onClick={() => onReveal(file.path)}
        >
          <Folder aria-hidden="true" className="size-2.5" />
          Show original
        </Button>
        <Button
          variant="outline"
          size="xs"
          className="h-6 rounded-[6px] px-2 text-[10px]"
          disabled={file.outputPath === null}
          onClick={() => {
            if (file.outputPath !== null) onReveal(file.outputPath)
          }}
        >
          <FolderOpen aria-hidden="true" className="size-2.5" />
          Show compressed
        </Button>
      </div>
    </div>
  )
}
