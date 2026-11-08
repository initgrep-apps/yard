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

import { Download } from 'lucide-react'
import { Progress } from '@/components/ui/progress'
import { formatSize } from '@/lib/sizeFormat'

export interface UpdateToastProps {
  version: string
  percent: number | null
  downloaded: number
  total: number | null
}

export function UpdateToast({ version, percent, downloaded, total }: UpdateToastProps) {
  return (
    <div className="flex w-80 flex-col gap-2 rounded-lg bg-popover p-3 text-popover-foreground shadow-md ring-1 ring-foreground/10">
      <div className="flex items-center gap-2">
        <Download data-icon="inline-start" className="size-4" aria-hidden="true" />
        <p className="text-sm font-medium">Downloading update</p>
      </div>
      <p className="text-xs text-muted-foreground">
        {percent === null ? `Version ${version}` : `Version ${version} · ${percent}%`}
      </p>
      <p className="text-xs text-muted-foreground">
        {total === null
          ? formatSize(downloaded)
          : `${formatSize(downloaded)} of ${formatSize(total)}`}
      </p>
      <Progress value={percent} aria-label="Update download progress" />
    </div>
  )
}
