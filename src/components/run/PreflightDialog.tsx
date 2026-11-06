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

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { answerPreflight, useConsent } from '@/hooks/useConsent'

export function PreflightDialog() {
  const { preflight } = useConsent()

  return (
    <Dialog
      open={preflight !== null}
      onOpenChange={(open) => {
        if (!open) answerPreflight(false)
      }}
    >
      <DialogContent showCloseButton={false} data-testid="preflight-dialog">
        <DialogHeader>
          <DialogTitle>Permission needed</DialogTitle>
          <DialogDescription>
            {preflight?.fileCount ?? 0} files need your permission before this batch can
            start.
          </DialogDescription>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">
          These files will change format (HEIC → JPEG). Allow once for this batch, or cancel
          and adjust your settings.
        </p>
        <DialogFooter>
          <Button variant="outline" onClick={() => answerPreflight(false)}>
            Cancel
          </Button>
          <Button onClick={() => answerPreflight(true)}>Allow for this batch</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
