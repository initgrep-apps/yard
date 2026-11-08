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

import { openUrl } from "@tauri-apps/plugin-opener"
import { version } from "../../../package.json"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemTitle,
} from "@/components/ui/item"
import { Progress, ProgressLabel, ProgressValue } from "@/components/ui/progress"
import { Spinner } from "@/components/ui/spinner"
import { formatSize } from "@/lib/sizeFormat"
import { useUpdate } from "@/providers/UpdateProvider"

const RELEASES_URL = "https://github.com/initgrep-apps/yard/releases"

function releaseTagUrl(releaseVersion: string): string {
  return `${RELEASES_URL}/tag/v${releaseVersion}`
}

function formatReleaseDate(date: string): string {
  return new Date(date).toLocaleDateString("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  })
}

export function UpdateSection(): React.ReactElement {
  const { state, canCheck, isCompressing, check, startDownload, install, decline, retry } =
    useUpdate()

  const update = "update" in state && state.update ? state.update : null

  const rowAction =
    state.status === "idle" || state.status === "checking" ? (
      <Button
        size="sm"
        onClick={check}
        disabled={!canCheck || state.status === "checking"}
      >
        {state.status === "checking" && <Spinner data-icon="inline-start" />}
        Check for updates
      </Button>
    ) : state.status === "current" ? (
      <Button size="sm" onClick={check} disabled={!canCheck}>
        Check again
      </Button>
    ) : null

  const rowBadge =
    state.status === "current"
      ? "Up to date"
      : state.status === "available"
        ? "Available"
        : state.status === "ready"
          ? "Ready"
          : null

  return (
    <div className="flex flex-col gap-2">
      <Item variant="outline" size="sm">
        <ItemContent>
          <ItemTitle>Version {version}</ItemTitle>
        </ItemContent>
        <ItemActions>
          {rowBadge && <Badge variant="secondary">{rowBadge}</Badge>}
          {rowAction}
        </ItemActions>
      </Item>

      {state.status === "checking" && (
        <p className="text-muted-foreground text-xs" aria-live="polite">
          Checking for updates…
        </p>
      )}

      {state.status === "current" && (
        <p className="text-muted-foreground text-xs" aria-live="polite">
          You're on the latest version.
        </p>
      )}

      {state.status === "idle" && !canCheck && (
        <p className="text-muted-foreground text-xs">
          Updates are disabled in development builds.
        </p>
      )}

      {state.status === "available" && update && (
        <Item variant="outline">
          <ItemContent>
            <ItemTitle>Version {update.version} is available</ItemTitle>
            {update.date && <ItemDescription>Released {formatReleaseDate(update.date)}.</ItemDescription>}
          </ItemContent>
          <ItemActions className="basis-full justify-end">
            <Button
              variant="link"
              size="sm"
              onClick={() => void openUrl(releaseTagUrl(update.version))}
            >
              What's new
            </Button>
            <Button variant="ghost" size="sm" onClick={decline}>
              Not now
            </Button>
            <Button size="sm" onClick={startDownload}>
              Update now
            </Button>
          </ItemActions>
          <ItemContent>
            <ItemDescription>
              The app will restart after installing. Staged files that haven't run will be
              cleared.
            </ItemDescription>
          </ItemContent>
        </Item>
      )}

      {state.status === "downloading" && update && (
        <Item variant="outline">
          <ItemContent>
            <Progress value={downloadPercent(state.downloaded, state.total)}>
              <ProgressLabel aria-live="polite">Downloading update…</ProgressLabel>
              <ProgressValue>
                {() => downloadProgressText(state.downloaded, state.total)}
              </ProgressValue>
            </Progress>
            <ItemDescription>
              You can close this window. The download continues in the background.
            </ItemDescription>
          </ItemContent>
        </Item>
      )}

      {state.status === "ready" && update && (
        <Item variant="outline">
          <ItemContent>
            <ItemTitle aria-live="polite">Update ready.</ItemTitle>
            <ItemDescription>
              Restart the app to finish installing version {update.version}.
            </ItemDescription>
            {isCompressing && (
              <ItemDescription>Finish the current batch to restart.</ItemDescription>
            )}
          </ItemContent>
          <ItemActions>
            <Button size="sm" onClick={install} disabled={isCompressing}>
              Restart app
            </Button>
          </ItemActions>
        </Item>
      )}

      {state.status === "installing" && update && (
        <Item variant="outline">
          <ItemContent>
            <div className="flex items-center gap-2" aria-live="polite">
              <Spinner />
              <span className="text-sm font-medium">Installing update…</span>
            </div>
            <ItemDescription>The app will restart.</ItemDescription>
            <Progress value={100} aria-label="Installing update progress" />
          </ItemContent>
        </Item>
      )}

      {state.status === "error" && (
        <div className="flex flex-col gap-2">
          <Alert variant="destructive">
            <AlertDescription>{state.message}</AlertDescription>
          </Alert>
          <ItemActions className="justify-end">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void openUrl(`${RELEASES_URL}/latest`)}
            >
              Download from GitHub
            </Button>
            <Button size="sm" onClick={retry}>
              Try again
            </Button>
          </ItemActions>
        </div>
      )}
    </div>
  )
}

function downloadPercent(downloaded: number, total: number | null): number | null {
  if (total === null || total <= 0) return null
  return Math.min(100, Math.round((downloaded / total) * 100))
}

function downloadProgressText(downloaded: number, total: number | null): string {
  const percent = downloadPercent(downloaded, total)
  if (percent === null || total === null) return formatSize(downloaded)
  return `${percent}% · ${formatSize(downloaded)} of ${formatSize(total)}`
}
