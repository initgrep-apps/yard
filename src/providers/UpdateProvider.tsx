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
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import {
  check as checkForUpdate,
  type DownloadEvent,
  type Update,
} from "@tauri-apps/plugin-updater"
import { relaunch } from "@tauri-apps/plugin-process"
import { toast } from "sonner"
import { UpdateToast } from "@/components/update/UpdateToast"
import { useCompressionContext } from "@/providers/CompressionProvider"
import { usePlatform } from "@/providers/PlatformProvider"

const CHECK_ERROR = "Couldn't check for updates. Check your connection and try again."
const DOWNLOAD_ERROR =
  "The update couldn't be downloaded. Check your connection and try again."
const INSTALL_ERROR =
  "The update couldn't be installed. Try again, or download the latest installer from GitHub."
const INSTALL_PATH_ERROR =
  "The app couldn't be replaced. Move it to Applications, then try again."

const PROGRESS_TOAST_ID = "updater-progress"
const ERROR_TOAST_ID = "updater-error"
const TOAST_THROTTLE_MS = 250

function downloadPercent(downloaded: number, total: number | null): number | null {
  if (total === null || total <= 0) return null
  return Math.min(100, Math.round((downloaded / total) * 100))
}

export type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "current" }
  | { status: "available"; update: Update }
  | { status: "downloading"; update: Update; downloaded: number; total: number | null }
  | { status: "ready"; update: Update }
  | { status: "installing"; update: Update }
  | {
      status: "error"
      stage: "check" | "download" | "install"
      message: string
      update: Update | null
    }

export interface UpdateContextValue {
  state: UpdateState
  canCheck: boolean
  isCompressing: boolean
  check: () => Promise<void>
  startDownload: () => Promise<void>
  install: () => Promise<void>
  decline: () => Promise<void>
  retry: () => Promise<void>
  setSurfaceOpen: (open: boolean) => void
}

export const UpdateContext = createContext<UpdateContextValue | null>(null)

export function useUpdate(): UpdateContextValue {
  const ctx = useContext(UpdateContext)
  if (!ctx) {
    throw new Error("useUpdate must be used within UpdateProvider")
  }
  return ctx
}

export function UpdateProvider({ children }: { children: ReactNode }) {
  const { isCompressing } = useCompressionContext()
  const platform = usePlatform()

  const [state, setState] = useState<UpdateState>({ status: "idle" })
  const [surfaceOpen, setSurfaceOpenState] = useState(false)

  const updateRef = useRef<Update | null>(null)
  const stateRef = useRef(state)
  stateRef.current = state
  const isCompressingRef = useRef(isCompressing)
  isCompressingRef.current = isCompressing

  const setSurfaceOpen = useCallback((open: boolean) => {
    setSurfaceOpenState(open)
  }, [])

  const installUpdate = useCallback(
    async (update: Update) => {
      if (isCompressingRef.current) {
        setState({ status: "ready", update })
        return
      }
      setState({ status: "installing", update })
      try {
        await update.install()
        if (platform === "darwin") {
          await relaunch()
        }
      } catch (error) {
        const message = /permission|denied|read-only/i.test(String(error))
          ? INSTALL_PATH_ERROR
          : INSTALL_ERROR
        setState({ status: "error", stage: "install", message, update })
      }
    },
    [platform],
  )

  const downloadUpdate = useCallback(
    async (update: Update) => {
      setState({ status: "downloading", update, downloaded: 0, total: null })
      try {
        await update.download(
          (event: DownloadEvent) => {
            if (event.event === "Started") {
              setState((s) =>
                s.status === "downloading"
                  ? { ...s, total: event.data.contentLength ?? null }
                  : s,
              )
            } else if (event.event === "Progress") {
              setState((s) =>
                s.status === "downloading"
                  ? { ...s, downloaded: s.downloaded + event.data.chunkLength }
                  : s,
              )
            }
          },
          { timeout: 600_000 },
        )
        await installUpdate(update)
      } catch {
        setState({ status: "error", stage: "download", message: DOWNLOAD_ERROR, update })
      }
    },
    [installUpdate],
  )

  const checkUpdate = useCallback(async () => {
    const held = updateRef.current
    if (held) {
      updateRef.current = null
      await held.close().catch(() => {})
    }
    setState({ status: "checking" })
    try {
      const update = await checkForUpdate({ timeout: 15_000 })
      if (update) {
        updateRef.current = update
        setState({ status: "available", update })
      } else {
        setState({ status: "current" })
      }
    } catch {
      setState({ status: "error", stage: "check", message: CHECK_ERROR, update: null })
    }
  }, [])

  const check = useCallback(async () => {
    const s = stateRef.current
    if (s.status === "checking" || s.status === "downloading" || s.status === "installing") {
      return
    }
    await checkUpdate()
  }, [checkUpdate])

  const startDownload = useCallback(async () => {
    const s = stateRef.current
    if (s.status !== "available") return
    await downloadUpdate(s.update)
  }, [downloadUpdate])

  const install = useCallback(async () => {
    const s = stateRef.current
    if (s.status !== "ready") return
    await installUpdate(s.update)
  }, [installUpdate])

  const decline = useCallback(async () => {
    const held = updateRef.current
    if (held) {
      updateRef.current = null
      await held.close().catch(() => {})
    }
    setState({ status: "idle" })
  }, [])

  const retry = useCallback(async () => {
    const s = stateRef.current
    if (s.status !== "error") return
    if (s.stage === "check") {
      await checkUpdate()
    } else if (s.stage === "download" && s.update) {
      await downloadUpdate(s.update)
    } else if (s.stage === "install" && s.update) {
      await installUpdate(s.update)
    }
  }, [checkUpdate, downloadUpdate, installUpdate])

  const lastToastStatusRef = useRef<UpdateState["status"] | null>(null)
  const lastToastAtRef = useRef(0)
  const throttleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    const renderProgress = () => {
      const s = stateRef.current
      if (s.status !== "downloading" && s.status !== "ready" && s.status !== "installing") {
        return
      }
      toast.custom(
        () => (
          <UpdateToast
            status={s.status}
            version={s.update.version}
            percent={s.status === "downloading" ? downloadPercent(s.downloaded, s.total) : null}
            downloaded={s.status === "downloading" ? s.downloaded : 0}
            total={s.status === "downloading" ? s.total : null}
          />
        ),
        { id: PROGRESS_TOAST_ID, position: "top-right", duration: Infinity },
      )
    }

    if (surfaceOpen) {
      lastToastStatusRef.current = null
      toast.dismiss(PROGRESS_TOAST_ID)
      toast.dismiss(ERROR_TOAST_ID)
    } else if (state.status === "error") {
      lastToastStatusRef.current = state.status
      toast.dismiss(PROGRESS_TOAST_ID)
      toast.error("Update failed", {
        id: ERROR_TOAST_ID,
        description: state.message,
        action: { label: "Retry", onClick: () => void retry() },
      })
    } else {
      toast.dismiss(ERROR_TOAST_ID)

      const status = state.status
      if (status !== "downloading" && status !== "ready" && status !== "installing") {
        lastToastStatusRef.current = status
        toast.dismiss(PROGRESS_TOAST_ID)
      } else {
        const elapsed = Date.now() - lastToastAtRef.current
        if (lastToastStatusRef.current !== status || elapsed >= TOAST_THROTTLE_MS) {
          lastToastStatusRef.current = status
          lastToastAtRef.current = Date.now()
          renderProgress()
        } else {
          throttleTimerRef.current = setTimeout(() => {
            throttleTimerRef.current = null
            lastToastAtRef.current = Date.now()
            renderProgress()
          }, TOAST_THROTTLE_MS - elapsed)
        }
      }
    }

    return () => {
      if (throttleTimerRef.current !== null) {
        clearTimeout(throttleTimerRef.current)
        throttleTimerRef.current = null
      }
    }
  }, [state, surfaceOpen, retry])

  const value = useMemo<UpdateContextValue>(
    () => ({
      state,
      canCheck: !import.meta.env.DEV,
      isCompressing,
      check,
      startDownload,
      install,
      decline,
      retry,
      setSurfaceOpen,
    }),
    [state, isCompressing, check, startDownload, install, decline, retry],
  )

  return <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>
}
