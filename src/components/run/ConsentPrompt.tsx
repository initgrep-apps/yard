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

import { useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { ConsentRequest, ConsentScope } from '@/hooks/useConsent'
import type { Change } from '@/types'

const CHANGE_LABELS: Record<Change, string> = {
  lossy_encode: 'Lossy encode',
  downscale: 'Downscale',
  drop_stream: 'Drop stream',
  format_change: 'Format change',
  metadata_strip: 'Metadata strip',
  rasterize: 'Rasterize',
  page_drop: 'Page drop',
}

export interface ConsentPromptProps {
  request: ConsentRequest
  onAnswer: (requestId: string, allow: boolean, scope: ConsentScope) => void
}

export function ConsentPrompt({ request, onAnswer }: ConsentPromptProps) {
  const [applyToBatch, setApplyToBatch] = useState(false)
  const scope: ConsentScope = applyToBatch ? 'batch' : 'once'

  return (
    <div
      data-testid="consent-prompt"
      role="group"
      aria-label="Permission needed"
      className="space-y-2 rounded-md border border-amber-500/30 bg-amber-500/10 p-2"
    >
      <p className="text-xs font-medium text-amber-700 dark:text-amber-400">
        {request.detail}
      </p>
      <div className="flex flex-wrap gap-1">
        {request.changes.map((change) => (
          <Badge key={change} variant="secondary" className="rounded text-[10px]">
            {CHANGE_LABELS[change] ?? change}
          </Badge>
        ))}
      </div>
      <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <input
          type="checkbox"
          className="size-3 accent-primary"
          checked={applyToBatch}
          onChange={(event) => setApplyToBatch(event.target.checked)}
        />
        Apply to the rest of this batch
      </label>
      <div className="flex gap-1.5">
        <Button size="xs" onClick={() => onAnswer(request.requestId, true, scope)}>
          Allow
        </Button>
        <Button
          size="xs"
          variant="outline"
          onClick={() => onAnswer(request.requestId, false, scope)}
        >
          Skip
        </Button>
      </div>
    </div>
  )
}
