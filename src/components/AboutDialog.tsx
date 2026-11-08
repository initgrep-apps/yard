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

import { useEffect, useState } from "react"
import thirdPartyNotices from "../../THIRD_PARTY_LICENSES.md?raw"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { ScrollArea } from "@/components/ui/scroll-area"
import { UpdateSection } from "@/components/update/UpdateSection"

const LICENSE_ROWS = [
  { name: "Yard", license: "GPL-3.0" },
  { name: "FFmpeg / ffprobe", license: "GPL-2.0-or-later" },
  { name: "Ghostscript", license: "AGPL-3.0" },
  { name: "libvips", license: "LGPL-3.0" },
  { name: "qpdf", license: "Apache-2.0" },
]

interface AboutDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function AboutDialog({ open, onOpenChange }: AboutDialogProps) {
  const [showNotices, setShowNotices] = useState(false)

  useEffect(() => {
    if (!open) setShowNotices(false)
  }, [open])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto">
        <DialogHeader className="items-center text-center">
          <img
            src="/favicon.png"
            alt=""
            data-testid="app-icon"
            className="size-12 rounded-2xl object-cover"
          />
          <DialogTitle>Yard</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-1 text-center">
          <p className="text-muted-foreground text-sm">
            Offline file compression tool
          </p>
          <UpdateSection />
        </div>
        <ul className="flex flex-col gap-2 text-sm">
          {LICENSE_ROWS.map((row) => (
            <li
              key={row.name}
              className="flex items-center justify-between gap-4"
            >
              <span>{row.name}</span>
              <span className="text-muted-foreground">{row.license}</span>
            </li>
          ))}
        </ul>
        <Button
          variant="link"
          className="self-center"
          aria-expanded={showNotices}
          aria-controls="third-party-notices"
          onClick={() => setShowNotices((shown) => !shown)}
        >
          Third-party notices
        </Button>
        {showNotices && (
          <ScrollArea
            id="third-party-notices"
            className="h-48 w-full min-w-0 rounded-xl border"
          >
            <pre
              data-testid="third-party-notices"
              className="p-3 whitespace-pre-wrap wrap-break-word font-mono text-xs"
            >
              {thirdPartyNotices}
            </pre>
          </ScrollArea>
        )}
      </DialogContent>
    </Dialog>
  );
}
