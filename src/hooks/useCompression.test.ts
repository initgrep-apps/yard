import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { StrictMode, useState } from 'react'

type EventHandler = (e: { payload: unknown }) => void

const registry = new Map<string, Set<EventHandler>>()
const unlistens: ReturnType<typeof vi.fn>[] = []

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: EventHandler) => {
    if (!registry.has(event)) registry.set(event, new Set())
    registry.get(event)!.add(handler)
    const unlisten = vi.fn(() => {
      registry.get(event)?.delete(handler)
    })
    unlistens.push(unlisten)
    return Promise.resolve(unlisten)
  }),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}))

import { useCompression, type CompressionCallbacks } from '@/hooks/useCompression'
import { invoke } from '@tauri-apps/api/core'
import { toast } from 'sonner'
import type { FileEntry } from '@/types'

const WATCHDOG_MS = 35 * 60 * 1000

const emit = (event: string, payload: unknown) =>
  act(() => {
    registry.get(event)?.forEach((handler) => handler({ payload }))
  })

const flush = () => act(async () => {})

function entry(overrides: Partial<FileEntry> = {}): FileEntry {
  return {
    id: 'f1',
    name: 'a.jpg',
    path: '/t/a.jpg',
    category: 'image',
    targetBytes: 204800,
    originalSize: 1024,
    status: 'pending',
    compressedSize: null,
    outputPath: null,
    error: null,
    strategyUsed: null,
    qualityUsed: null,
    ...overrides,
  }
}

function setup(
  files: FileEntry[],
  callbacks?: CompressionCallbacks,
  wrapper?: typeof StrictMode
) {
  return renderHook(
    (props: { files: FileEntry[] }) => {
      const [state, setFiles] = useState<FileEntry[]>(props.files)
      const comp = useCompression(state, setFiles, callbacks)
      return { comp, files: state }
    },
    {
      initialProps: { files },
      ...(wrapper ? { wrapper } : {}),
    }
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  registry.clear()
  unlistens.length = 0
})

describe('useCompression event handling', () => {
  it('file:progress stores percent and flips pending rows to compressing', async () => {
    const { result } = setup([entry()])
    await flush()
    emit('file:progress', { fileId: 'f1', percent: 45 })
    expect(result.current.comp.progressByFile['f1']).toBe(45)
    expect(result.current.files[0].status).toBe('compressing')
  })

  it('file:progress for unknown fileId is ignored', async () => {
    const { result } = setup([entry()])
    await flush()
    emit('file:progress', { fileId: 'ghost', percent: 10 })
    expect(result.current.comp.progressByFile['ghost']).toBeUndefined()
    expect(result.current.files[0].status).toBe('pending')
  })

  it('file:progress for non-active rows leaves status and progress untouched', async () => {
    const { result } = setup([
      entry({ status: 'done', compressedSize: 500, outputPath: '/o/a.jpg' }),
    ])
    await flush()
    emit('file:progress', { fileId: 'f1', percent: 80 })
    expect(result.current.comp.progressByFile['f1']).toBeUndefined()
    expect(result.current.files[0].status).toBe('done')
  })

  it('file:progress after completion does not re-add a stale percent', async () => {
    const { result } = setup([entry()])
    await flush()
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '/o/a.jpg',
      originalSize: 1024,
      compressedSize: 500,
      status: 'done',
      metTarget: true,
    })
    emit('file:progress', { fileId: 'f1', percent: 80 })
    expect(result.current.comp.progressByFile['f1']).toBeUndefined()
    expect(result.current.files[0].status).toBe('done')
  })

  it('file:complete applies status, sizes, output path and clears progress', async () => {
    const { result } = setup([entry()])
    await flush()
    emit('file:progress', { fileId: 'f1', percent: 40 })
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '/o/a.jpg',
      originalSize: 1024,
      compressedSize: 500,
      status: 'done',
      metTarget: true,
    })
    expect(result.current.comp.progressByFile['f1']).toBeUndefined()
    expect(result.current.files[0]).toMatchObject({
      status: 'done',
      compressedSize: 500,
      outputPath: '/o/a.jpg',
    })
  })

  it('needs_permission keeps no output and surfaces the reason', async () => {
    const { result } = setup([entry()])
    await flush()
    emit('file:progress', { fileId: 'f1', percent: 40 })
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '',
      originalSize: 1024,
      compressedSize: 0,
      status: 'needs_permission',
      metTarget: false,
      reason: 'Needs permission: downscale',
    })
    expect(result.current.comp.progressByFile['f1']).toBeUndefined()
    expect(result.current.files[0]).toMatchObject({
      status: 'needs_permission',
      compressedSize: null,
      outputPath: null,
      error: 'Needs permission: downscale',
    })
  })

  it('file:error sets failed with message and resets stale strategy fields', async () => {
    const { result } = setup([entry({ strategyUsed: 'stale=old', qualityUsed: 7 })])
    await flush()
    emit('file:progress', { fileId: 'f1', percent: 20 })
    emit('file:error', { fileId: 'f1', errorMessage: 'Cannot read file: locked' })
    expect(result.current.comp.progressByFile['f1']).toBeUndefined()
    expect(result.current.files[0]).toMatchObject({
      status: 'failed',
      error: 'Cannot read file: locked',
      strategyUsed: null,
      qualityUsed: null,
    })
  })

  it('file:cancelled sets cancelled status and resets stale strategy fields', async () => {
    const { result } = setup([entry({ strategyUsed: 'stale=old', qualityUsed: 7 })])
    await flush()
    emit('file:progress', { fileId: 'f1', percent: 20 })
    emit('file:cancelled', { fileId: 'f1' })
    expect(result.current.comp.progressByFile['f1']).toBeUndefined()
    expect(result.current.files[0]).toMatchObject({
      status: 'cancelled',
      strategyUsed: null,
      qualityUsed: null,
    })
  })

  it('terminal events for unknown fileIds leave state unchanged', async () => {
    const { result } = setup([entry()])
    await flush()
    emit('file:complete', { fileId: 'ghost', outputPath: '/o', compressedSize: 1, status: 'done' })
    emit('file:error', { fileId: 'ghost', errorMessage: 'x' })
    emit('file:cancelled', { fileId: 'ghost' })
    expect(result.current.files[0]).toMatchObject({ status: 'pending', error: null })
    expect(result.current.comp.progressByFile).toEqual({})
  })

  it('batch:complete resets isCompressing and fires onBatchComplete', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(result.current.comp.isCompressing).toBe(true)
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '/o/a.jpg',
      originalSize: 1024,
      compressedSize: 500,
      status: 'done',
      metTarget: true,
    })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 1024 })
    expect(result.current.comp.isCompressing).toBe(false)
    expect(onBatchComplete).toHaveBeenCalledWith(1024, 1, 0, false)
  })

  it('batch:error resets isCompressing and fires onBatchError', async () => {
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(result.current.comp.isCompressing).toBe(true)
    emit('batch:error', { batchId: 'b1', errorMessage: 'Output folder not writable' })
    expect(result.current.comp.isCompressing).toBe(false)
    expect(onBatchError).toHaveBeenCalledWith('Output folder not writable')
  })

  it('terminal events for unknown fileIds do not inflate the processed count', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:complete', { fileId: 'ghost', outputPath: '/o', compressedSize: 1, status: 'done' })
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '/o/a.jpg',
      originalSize: 10,
      compressedSize: 5,
      status: 'done',
      metTarget: true,
    })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5 })
    expect(onBatchComplete).toHaveBeenCalledWith(5, 1, 0, false)
  })

  it('cancelled files are not counted and a zero-count batch suppresses the success toast', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:cancelled', { fileId: 'f1' })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 0 })
    expect(result.current.comp.isCompressing).toBe(false)
    expect(onBatchComplete).not.toHaveBeenCalled()
  })

  it('mixed batch counts only non-cancelled files', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup(
      [entry({ id: 'f1' }), entry({ id: 'f2', name: 'b.jpg' })],
      { onBatchComplete }
    )
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:cancelled', { fileId: 'f1' })
    emit('file:complete', {
      fileId: 'f2',
      outputPath: '/o/b.jpg',
      originalSize: 10,
      compressedSize: 5,
      status: 'done',
      metTarget: true,
    })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5 })
    expect(onBatchComplete).toHaveBeenCalledWith(5, 1, 0, false)
  })

  it('processed count resets between batches', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup(
      [entry({ id: 'f1' }), entry({ id: 'f2', name: 'b.jpg' })],
      { onBatchComplete }
    )
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '/o/a.jpg',
      originalSize: 10,
      compressedSize: 5,
      status: 'done',
      metTarget: true,
    })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5 })
    expect(onBatchComplete).toHaveBeenLastCalledWith(5, 1, 0, false)
    emit('file:complete', {
      fileId: 'f2',
      outputPath: '/o/b.jpg',
      originalSize: 10,
      compressedSize: 3,
      status: 'done',
      metTarget: true,
    })
    emit('batch:complete', { batchId: 'b2', totalSavedBytes: 7 })
    expect(onBatchComplete).toHaveBeenLastCalledWith(7, 1, 0, false)
  })

  it('error count accumulates file:error events and reaches onBatchComplete', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup(
      [entry({ id: 'f1' }), entry({ id: 'f2', name: 'b.jpg' }), entry({ id: 'f3', name: 'c.jpg' })],
      { onBatchComplete }
    )
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:error', { fileId: 'f1', errorMessage: 'boom' })
    emit('file:error', { fileId: 'f2', errorMessage: 'boom' })
    emit('file:complete', {
      fileId: 'f3',
      outputPath: '/o/c.jpg',
      originalSize: 10,
      compressedSize: 5,
      status: 'done',
      metTarget: true,
    })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5 })
    expect(onBatchComplete).toHaveBeenCalledWith(5, 3, 2, false)
  })

  it('error count ignores unknown fileIds', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:error', { fileId: 'ghost', errorMessage: 'boom' })
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '/o/a.jpg',
      originalSize: 10,
      compressedSize: 5,
      status: 'done',
      metTarget: true,
    })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5 })
    expect(onBatchComplete).toHaveBeenCalledWith(5, 1, 0, false)
  })

  it('error count resets between batches', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry({ id: 'f1' }), entry({ id: 'f2', name: 'b.jpg' })], {
      onBatchComplete,
    })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:error', { fileId: 'f1', errorMessage: 'boom' })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 0 })
    expect(onBatchComplete).toHaveBeenLastCalledWith(0, 1, 1, false)
    emit('file:complete', {
      fileId: 'f2',
      outputPath: '/o/b.jpg',
      originalSize: 10,
      compressedSize: 3,
      status: 'done',
      metTarget: true,
    })
    emit('batch:complete', { batchId: 'b2', totalSavedBytes: 7 })
    expect(onBatchComplete).toHaveBeenLastCalledWith(7, 1, 0, false)
  })
})

describe('useCompression batch isolation', () => {
  it('batch events carrying a stale batchId are ignored while a run is active', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('b2')
    const onBatchError = vi.fn()
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchError, onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(result.current.comp.isCompressing).toBe(true)
    emit('batch:error', { batchId: 'b1', errorMessage: 'stale failure' })
    expect(result.current.comp.isCompressing).toBe(true)
    expect(onBatchError).not.toHaveBeenCalled()
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 99 })
    expect(result.current.comp.isCompressing).toBe(true)
    expect(onBatchComplete).not.toHaveBeenCalled()
  })

  it('stale cancelled batch:complete is ignored', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('b2')
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 99, cancelled: true })
    expect(result.current.comp.isCompressing).toBe(true)
    expect(toast.info).not.toHaveBeenCalled()
    expect(onBatchComplete).not.toHaveBeenCalled()
  })

  it('duplicate batch:complete events are processed exactly once', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '/o/a.jpg',
      originalSize: 10,
      compressedSize: 5,
      status: 'done',
      metTarget: true,
    })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5, cancelled: true })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5, cancelled: true })
    expect(toast.info).toHaveBeenCalledTimes(1)
    expect(onBatchComplete).toHaveBeenCalledTimes(1)
    expect(onBatchComplete).toHaveBeenCalledWith(5, 1, 0, true)
  })

  it('batch events carrying the active batchId are processed normally', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('b2')
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('batch:error', { batchId: 'b1', errorMessage: 'stale failure' })
    emit('batch:error', { batchId: 'b2', errorMessage: 'real failure' })
    expect(result.current.comp.isCompressing).toBe(false)
    expect(onBatchError).toHaveBeenCalledWith('real failure')
  })
})

describe('useCompression commands', () => {
  it('compress_all invokes command when pending files exist', async () => {
    const { result } = setup([entry()])
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(invoke).toHaveBeenCalledWith('compress_all')
    expect(result.current.comp.isCompressing).toBe(true)
  })

  it('compress_all no-ops when no pending files', async () => {
    const { result } = setup([entry({ status: 'done' })])
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(invoke).not.toHaveBeenCalled()
    expect(result.current.comp.isCompressing).toBe(false)
  })

  it('cancel_file / cancel_all / reveal_in_folder invoke with camelCase payloads', async () => {
    const { result } = setup([entry()])
    await flush()
    await act(() => result.current.comp.cancelFile('f1'))
    expect(invoke).toHaveBeenCalledWith('cancel_file', { fileId: 'f1' })
    await act(() => result.current.comp.cancelAll())
    expect(invoke).toHaveBeenCalledWith('cancel_all')
    await act(() => result.current.comp.revealInFolder('/out/a.jpg'))
    expect(invoke).toHaveBeenCalledWith('reveal_in_folder', { filePath: '/out/a.jpg' })
  })

  it('compress_all resets state when backend returns empty batch id', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('')
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(result.current.comp.isCompressing).toBe(false)
    expect(onBatchError).toHaveBeenCalledTimes(1)
    expect(onBatchError).toHaveBeenCalledWith(expect.stringMatching(/\S/))
  })

  it('compress_all resets state and reports when invoke rejects', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('worker dead'))
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(result.current.comp.isCompressing).toBe(false)
    expect(onBatchError).toHaveBeenCalledWith('worker dead')
  })

  it('reveal failure surfaces a toast error', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('opener unavailable'))
    const { result } = setup([entry()])
    await flush()
    await act(() => result.current.comp.revealInFolder('/out/a.jpg'))
    expect(toast.error).toHaveBeenCalledWith('Could not open folder: opener unavailable')
  })

  it('cancel_all failure surfaces a toast error', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('ipc down'))
    const { result } = setup([entry()])
    await flush()
    await act(() => result.current.comp.cancelAll())
    expect(toast.error).toHaveBeenCalledWith('Could not cancel: ipc down')
  })

  it('cancel_file failure surfaces a toast error', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('ipc down'))
    const { result } = setup([entry()])
    await flush()
    await act(() => result.current.comp.cancelFile('f1'))
    expect(toast.error).toHaveBeenCalledWith('Could not cancel file: ipc down')
  })
})

describe('useCompression listener lifecycle', () => {
  it('subscribes to all six events', async () => {
    setup([entry()])
    await flush()
    expect([...registry.keys()].sort()).toEqual([
      'batch:complete',
      'batch:error',
      'file:cancelled',
      'file:complete',
      'file:error',
      'file:progress',
    ])
    expect([...registry.values()].every((set) => set.size === 1)).toBe(true)
  })

  it('unsubscribes all listeners on unmount', async () => {
    const { unmount } = setup([entry()])
    await flush()
    expect(registry.size).toBe(6)
    unmount()
    expect(unlistens.length).toBeGreaterThan(0)
    expect(unlistens.every((u) => u.mock.calls.length === 1)).toBe(true)
    expect([...registry.values()].every((set) => set.size === 0)).toBe(true)
  })

  it('strict mode double-mount does not leak duplicate listeners', async () => {
    const onBatchComplete = vi.fn()
    const { unmount } = setup([entry()], { onBatchComplete }, StrictMode)
    await flush()
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '/o/a.jpg',
      originalSize: 10,
      compressedSize: 5,
      status: 'done',
      metTarget: true,
    })
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5 })
    expect(onBatchComplete).toHaveBeenCalledTimes(1)
    expect(onBatchComplete).toHaveBeenCalledWith(5, 1, 0, false)
    unmount()
    await flush()
    expect(unlistens.length).toBeGreaterThan(0)
    expect(unlistens.every((u) => u.mock.calls.length === 1)).toBe(true)
    expect([...registry.values()].every((set) => set.size === 0)).toBe(true)
  })

  it('listener registration failure reports batch error', async () => {
    const { listen } = await import('@tauri-apps/api/event')
    vi.mocked(listen).mockRejectedValueOnce(new Error('bus down'))
    const onBatchError = vi.fn()
    setup([entry()], { onBatchError })
    await flush()
    expect(onBatchError).toHaveBeenCalledWith('Lost connection to compression worker')
  })
})

describe('useCompression strategy capture', () => {
  it('file:complete persists strategyUsed and qualityUsed onto the file entry', async () => {
    const { result } = setup([entry()])
    await flush()
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '/o/a.jpg',
      originalSize: 1024,
      compressedSize: 500,
      status: 'done',
      metTarget: true,
      strategyUsed: 'crf=24',
      qualityUsed: 24,
    })
    expect(result.current.files[0]).toMatchObject({
      strategyUsed: 'crf=24',
      qualityUsed: 24,
    })
  })

  it('file:complete without strategy fields defaults them to null', async () => {
    const { result } = setup([entry({ strategyUsed: 'stale=old', qualityUsed: 99 })])
    await flush()
    emit('file:complete', {
      fileId: 'f1',
      outputPath: '/o/a.jpg',
      originalSize: 1024,
      compressedSize: 500,
      status: 'done',
      metTarget: true,
    })
    expect(result.current.files[0]).toMatchObject({
      strategyUsed: null,
      qualityUsed: null,
    })
  })
})

describe('useCompression batch cancellation', () => {
  const completePayload = {
    fileId: 'f1',
    outputPath: '/o/a.jpg',
    originalSize: 10,
    compressedSize: 5,
    status: 'done',
    metTarget: true,
  }

  it('batch:complete without cancelled field falls back to the cancelled ref', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.cancelAll())
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 0 })
    expect(toast.info).toHaveBeenCalledWith('Batch cancelled')
    expect(onBatchComplete).toHaveBeenCalledWith(0, 0, 0, true)
  })

  it('batch:complete with cancelled payload shows exactly one Batch cancelled toast', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:complete', completePayload)
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5, cancelled: true })
    expect(toast.info).toHaveBeenCalledTimes(1)
    expect(toast.info).toHaveBeenCalledWith('Batch cancelled')
    expect(onBatchComplete).toHaveBeenCalledWith(5, 1, 0, true)
  })

  it('batch:complete with cancelled: false payload overrides a stale cancelled ref', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:complete', completePayload)
    await act(() => result.current.comp.cancelAll())
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5, cancelled: false })
    expect(toast.info).not.toHaveBeenCalled()
    expect(onBatchComplete).toHaveBeenCalledWith(5, 1, 0, false)
  })

  it('cancelAll with processed files still fires onBatchComplete and shows Batch cancelled toast', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:complete', completePayload)
    await act(() => result.current.comp.cancelAll())
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5 })
    expect(toast.info).toHaveBeenCalledWith('Batch cancelled')
    expect(onBatchComplete).toHaveBeenCalledWith(5, 1, 0, true)
  })

  it('batch:error after cancelAll shows Batch cancelled toast and resets the flag', async () => {
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    await act(() => result.current.comp.cancelAll())
    emit('batch:error', { batchId: 'b1', errorMessage: 'worker unavailable' })
    expect(toast.info).toHaveBeenCalledWith('Batch cancelled')
    expect(onBatchError).toHaveBeenCalledWith('worker unavailable')
    vi.mocked(toast.info).mockClear()
    emit('batch:complete', { batchId: 'b2', totalSavedBytes: 0 })
    expect(toast.info).not.toHaveBeenCalled()
  })

  it('compressAll after a cancelled batch runs to completion without cancelled toast', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup(
      [entry({ id: 'f1' }), entry({ id: 'f2', name: 'b.jpg' })],
      { onBatchComplete }
    )
    await flush()
    await act(() => result.current.comp.compressAll())
    await act(() => result.current.comp.cancelAll())
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 0 })
    expect(toast.info).toHaveBeenCalledWith('Batch cancelled')
    vi.mocked(toast.info).mockClear()
    await act(() => result.current.comp.compressAll())
    emit('file:complete', { ...completePayload, fileId: 'f2', outputPath: '/o/b.jpg' })
    emit('batch:complete', { batchId: 'b2', totalSavedBytes: 5 })
    expect(toast.info).not.toHaveBeenCalled()
    expect(onBatchComplete).toHaveBeenCalledWith(5, 1, 0, false)
  })

  it('cancelAll invoke rejection does not set the cancelled flag', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('ipc down'))
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.cancelAll())
    expect(toast.error).toHaveBeenCalledWith('Could not cancel: ipc down')
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 0 })
    expect(toast.info).not.toHaveBeenCalled()
    expect(onBatchComplete).not.toHaveBeenCalled()
  })

  it('batch:complete without cancel does not show cancelled toast', async () => {
    const onBatchComplete = vi.fn()
    const { result } = setup([entry()], { onBatchComplete })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:complete', completePayload)
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5 })
    expect(toast.info).not.toHaveBeenCalled()
    expect(onBatchComplete).toHaveBeenCalledWith(5, 1, 0, false)
  })
})

describe('useCompression terminal watchdog', () => {
  const completePayload = {
    fileId: 'f1',
    outputPath: '/o/a.jpg',
    originalSize: 10,
    compressedSize: 5,
    status: 'done',
    metTarget: true,
  }

  beforeEach(() => {
    vi.useFakeTimers()
    vi.mocked(invoke).mockReset()
    vi.mocked(invoke).mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('watchdog does not fire while batch terminal arrives in time', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('b1')
    const onBatchComplete = vi.fn()
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchComplete, onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(result.current.comp.isCompressing).toBe(true)
    expect(vi.getTimerCount()).toBe(1)
    emit('file:complete', completePayload)
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 5 })
    expect(result.current.comp.isCompressing).toBe(false)
    expect(vi.getTimerCount()).toBe(0)
    act(() => {
      vi.advanceTimersByTime(WATCHDOG_MS + 1000)
    })
    expect(toast.error).not.toHaveBeenCalled()
    expect(onBatchError).not.toHaveBeenCalled()
    expect(onBatchComplete).toHaveBeenCalledTimes(1)
  })

  it('watchdog recovers run state when the terminal event never arrives', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('b1')
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('file:progress', { fileId: 'f1', percent: 40 })
    expect(result.current.comp.progressByFile['f1']).toBe(40)
    act(() => {
      vi.advanceTimersByTime(WATCHDOG_MS + 1000)
    })
    expect(result.current.comp.isCompressing).toBe(false)
    expect(result.current.comp.progressByFile).toEqual({})
    expect(toast.error).not.toHaveBeenCalled()
    expect(onBatchError).toHaveBeenCalledTimes(1)
    expect(onBatchError).toHaveBeenCalledWith(
      'Compression status lost — check History for results'
    )
  })

  it('second compressAll clears a pending watchdog', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('b1').mockResolvedValueOnce('b2')
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(vi.getTimerCount()).toBe(1)
    await act(() => result.current.comp.compressAll())
    expect(vi.getTimerCount()).toBe(1)
    act(() => {
      vi.advanceTimersByTime(WATCHDOG_MS + 1000)
    })
    expect(toast.error).not.toHaveBeenCalled()
    expect(onBatchError).toHaveBeenCalledTimes(1)
    expect(onBatchError).toHaveBeenCalledWith(
      'Compression status lost — check History for results'
    )
    expect(result.current.comp.isCompressing).toBe(false)
  })

  it('invoke rejection never arms the watchdog', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('worker dead'))
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(result.current.comp.isCompressing).toBe(false)
    expect(onBatchError).toHaveBeenCalledWith('worker dead')
    expect(vi.getTimerCount()).toBe(0)
    act(() => {
      vi.advanceTimersByTime(WATCHDOG_MS + 1000)
    })
    expect(toast.error).not.toHaveBeenCalled()
    expect(onBatchError).toHaveBeenCalledTimes(1)
  })

  it('stale terminal events do not disarm the active run watchdog', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('b2')
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    emit('batch:complete', { batchId: 'b1', totalSavedBytes: 99 })
    emit('batch:error', { batchId: 'b1', errorMessage: 'stale failure' })
    expect(result.current.comp.isCompressing).toBe(true)
    expect(vi.getTimerCount()).toBe(1)
    act(() => {
      vi.advanceTimersByTime(WATCHDOG_MS + 1000)
    })
    expect(toast.error).not.toHaveBeenCalled()
    expect(onBatchError).toHaveBeenCalledWith(
      'Compression status lost — check History for results'
    )
  })

  it('unmount clears a pending watchdog', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('b1')
    const { result, unmount } = setup([entry()])
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(vi.getTimerCount()).toBe(1)
    unmount()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('file progress events keep the watchdog alive past the timeout', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('b1')
    const onBatchError = vi.fn()
    const { result } = setup([entry()], { onBatchError })
    await flush()
    await act(() => result.current.comp.compressAll())
    expect(result.current.comp.isCompressing).toBe(true)
    for (let i = 0; i < 20; i++) {
      emit('file:progress', { fileId: 'f1', percent: i * 5 })
      act(() => {
        vi.advanceTimersByTime(WATCHDOG_MS - 1000)
      })
    }
    expect(result.current.comp.isCompressing).toBe(true)
    expect(toast.error).not.toHaveBeenCalled()
    expect(onBatchError).not.toHaveBeenCalled()
    act(() => {
      vi.advanceTimersByTime(WATCHDOG_MS + 1000)
    })
    expect(toast.error).not.toHaveBeenCalled()
    expect(onBatchError).toHaveBeenCalledTimes(1)
    expect(onBatchError).toHaveBeenCalledWith(
      'Compression status lost — check History for results'
    )
    expect(result.current.comp.isCompressing).toBe(false)
    expect(result.current.comp.progressByFile).toEqual({})
  })
})
