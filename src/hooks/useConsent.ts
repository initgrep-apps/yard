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

import { useCallback, useMemo, useSyncExternalStore } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { Category, Change, ConsentSettings, FileEntry, Settings } from '@/types'

export type ConsentScope = 'once' | 'batch'

export interface ConsentRequest {
  requestId: string
  fileId: string
  category: Category
  changes: Change[]
  detail: string
}

export interface ConsentGrant {
  category: Category
  changes: Change[]
}

export interface PreflightPlan {
  fileCount: number
  grants: ConsentGrant[]
}

interface ConsentStoreState {
  requests: ConsentRequest[]
  preflight: PreflightPlan | null
}

const DEFAULT_CONSENT: ConsentSettings = { allowDestructive: false, heicToJpg: true }

let store: ConsentStoreState = { requests: [], preflight: null }
const listeners = new Set<() => void>()
let listenerStarted = false
let stopListener: (() => void) | null = null
let preflightResolver: ((grants: ConsentGrant[] | null) => void) | null = null

function notify(next: ConsentStoreState) {
  store = next
  listeners.forEach((listener) => listener())
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  startConsentListener()
  return () => {
    listeners.delete(listener)
  }
}

function getSnapshot() {
  return store
}

function startConsentListener() {
  if (listenerStarted) return
  listenerStarted = true
  const failed = (err: unknown) => {
    listenerStarted = false
    console.error('consent listener registration failed', err)
  }
  try {
    void listen<ConsentRequest>('consent:required', (event) => {
      const payload = event.payload
      notify({
        ...store,
        requests: [
          ...store.requests.filter((request) => request.requestId !== payload.requestId),
          payload,
        ],
      })
    })
      .then((stop) => {
        if (listenerStarted) stopListener = stop
        else stop()
      })
      .catch(failed)
  } catch (err) {
    failed(err)
  }
}

export function isHeicFile(file: FileEntry): boolean {
  return /\.(heic|heif)$/i.test(file.name)
}

export function planPreflight(
  files: FileEntry[],
  settings: ConsentSettings
): PreflightPlan | null {
  const heic = files.filter((file) => file.status === 'pending' && isHeicFile(file))
  if (heic.length === 0 || settings.allowDestructive) return null

  const needing = heic.filter(
    (file) => !(file.category === 'image' && settings.heicToJpg)
  )
  if (needing.length === 0) return null

  const grants = new Map<Category, Set<Change>>()
  for (const file of needing) {
    const changes = grants.get(file.category) ?? new Set<Change>()
    changes.add('format_change')
    grants.set(file.category, changes)
  }

  return {
    fileCount: needing.length,
    grants: Array.from(grants, ([category, changes]) => ({
      category,
      changes: Array.from(changes),
    })),
  }
}

export async function requestPreflight(files: FileEntry[]): Promise<ConsentGrant[] | null> {
  if (!files.some((file) => file.status === 'pending' && isHeicFile(file))) return []

  const settings = await invoke<Settings>('get_settings').catch(() => null)
  const plan = planPreflight(files, settings?.consent ?? DEFAULT_CONSENT)
  if (!plan) return []

  if (preflightResolver) {
    preflightResolver(null)
    preflightResolver = null
  }
  notify({ ...store, preflight: plan })
  return new Promise((resolve) => {
    preflightResolver = resolve
  })
}

export function answerPreflight(allow: boolean) {
  const plan = store.preflight
  const resolve = preflightResolver
  preflightResolver = null
  notify({ ...store, preflight: null })
  resolve?.(allow ? (plan?.grants ?? []) : null)
}

export function dropConsentForFile(fileId: string) {
  if (!store.requests.some((request) => request.fileId === fileId)) return
  notify({
    ...store,
    requests: store.requests.filter((request) => request.fileId !== fileId),
  })
}

export function clearConsentRequests() {
  if (store.requests.length === 0) return
  notify({ ...store, requests: [] })
}

export function useConsent() {
  const state = useSyncExternalStore(subscribe, getSnapshot, getSnapshot)

  const answerConsent = useCallback(
    async (requestId: string, allow: boolean, scope: ConsentScope) => {
      const answered = store.requests.find((request) => request.requestId === requestId)
      try {
        await invoke('answer_consent', { requestId, allow, scope })
      } catch (err) {
        console.error('answer_consent failed', err)
        return
      }
      notify({
        ...store,
        requests: store.requests.filter((request) => {
          if (request.requestId === requestId) return false
          if (!allow || scope !== 'batch' || !answered) return true
          return !(
            request.category === answered.category &&
            request.changes.every((change) => answered.changes.includes(change))
          )
        }),
      })
    },
    []
  )

  const requestFor = useCallback(
    (fileId: string) =>
      state.requests.find((request) => request.fileId === fileId) ?? null,
    [state.requests]
  )

  const waitingFileIds = useMemo(
    () => new Set(state.requests.map((request) => request.fileId)),
    [state.requests]
  )

  return {
    requests: state.requests,
    preflight: state.preflight,
    waitingCount: state.requests.length,
    waitingFileIds,
    requestFor,
    answerConsent,
  }
}

export function __resetConsentStore() {
  if (preflightResolver) {
    preflightResolver(null)
    preflightResolver = null
  }
  stopListener?.()
  stopListener = null
  listenerStarted = false
  store = { requests: [], preflight: null }
  listeners.forEach((listener) => listener())
}
