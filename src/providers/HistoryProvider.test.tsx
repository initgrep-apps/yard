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

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen } from '@testing-library/react'

const registry = new Map<string, Set<(e: { payload: unknown }) => void>>()
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
    if (!registry.has(event)) registry.set(event, new Set())
    registry.get(event)!.add(handler)
    return Promise.resolve(() => registry.get(event)?.delete(handler))
  }),
}))
vi.mock('sonner', () => ({ toast: { error: vi.fn() } }))

import { HistoryProvider, useHistoryContext } from '@/providers/HistoryProvider'
import { invoke } from '@tauri-apps/api/core'
import type { HistoryFilters, HistoryResponse, HistorySession } from '@/types'

function session(sessionId: string, fileName: string, startedAt: number): HistorySession {
  return {
    sessionId,
    startedAt,
    endedAt: startedAt + 1_000,
    outputFolder: '/out',
    fileCount: 1,
    files: [
      {
        name: fileName,
        path: `/in/${fileName}`,
        outputPath: null,
        category: 'other',
        targetBytes: 100,
        originalSize: 200,
        compressedSize: 100,
        status: 'done',
        qualityUsed: null,
        strategyUsed: null,
        error: null,
        durationMs: 10,
        completedAt: startedAt,
      },
    ],
    outcome: {
      done: 1,
      bestEffort: 0,
      failed: 0,
      cancelled: 0,
      unsupported: 0,
      alreadyUnderTarget: 0,
      needsPermission: 0,
    },
    totalSavedBytes: 100,
    cancelled: false,
    interrupted: false,
    matchedFiles: null,
  }
}

const sessions = [session('s1', 'alpha.png', 2_000), session('s2', 'beta.png', 1_000)]

const toResponse = (items: HistorySession[]): HistoryResponse => ({
  sessions: items,
  page: 0,
  pageSize: 50,
  totalSessions: items.length,
  totalFiles: items.length,
  summary: {
    sessions: items.length,
    files: items.length,
    savedBytes: 100 * items.length,
    failedFiles: 0,
  },
  outputFolders: ['/out'],
  skippedLines: 0,
})

function mockHistory() {
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'clear_history') return undefined
    const filters = (args as { filters?: HistoryFilters } | undefined)?.filters ?? {}
    const page = (args as { page?: number } | undefined)?.page ?? 0
    const matched = filters.search
      ? sessions.filter((item) => item.files.some((file) => file.name.includes(filters.search!)))
      : sessions
    return { ...toResponse(matched), page }
  })
}

const historyCalls = () =>
  vi.mocked(invoke).mock.calls.filter(([command]) => command === 'get_history')

const searchCalls = () =>
  historyCalls().filter(
    ([, args]) => (args as { filters?: HistoryFilters }).filters?.search !== undefined
  )

const lastFilters = () => {
  const calls = historyCalls()
  return (calls[calls.length - 1][1] as { filters: HistoryFilters }).filters
}

const displayedFilters = (): HistoryFilters =>
  JSON.parse(screen.getByTestId('filters').textContent ?? '{}')

const flush = () =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(0)
  })

const advance = (ms: number) =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(ms)
  })

function Probe() {
  const ctx = useHistoryContext()
  return (
    <div>
      <span data-testid="selected">{ctx.selectedSessionId ?? 'none'}</span>
      <span data-testid="expanded">{String(ctx.expanded)}</span>
      <span data-testid="search-text">{ctx.searchText}</span>
      <span data-testid="page">{ctx.page}</span>
      <span data-testid="loading">{String(ctx.loading)}</span>
      <span data-testid="filters">{JSON.stringify(ctx.filters)}</span>
      <span data-testid="error">{ctx.error ?? 'none'}</span>
      <button type="button" onClick={() => ctx.selectSession('s2')}>
        select-second
      </button>
      <button type="button" onClick={() => ctx.selectSession('missing-id')}>
        select-missing
      </button>
      <button type="button" onClick={() => ctx.setSearchText('  alpha  ')}>
        find-alpha
      </button>
      <button type="button" onClick={() => ctx.setSearchText('a')}>
        type-a
      </button>
      <button type="button" onClick={() => ctx.setSearchText('alpha')}>
        type-alpha
      </button>
      <button type="button" onClick={() => ctx.setSearchText('missing')}>
        type-missing
      </button>
      <button type="button" onClick={() => ctx.setSearchText('')}>
        clear-search
      </button>
      <button type="button" onClick={() => ctx.setFilters({ categories: ['image'] })}>
        apply-category
      </button>
      <button
        type="button"
        onClick={() => ctx.setFilters({ statuses: ['failed'], categories: ['image'] })}
      >
        apply-status
      </button>
      <button type="button" onClick={() => ctx.setFilters({ sort: 'newest' })}>
        clear-filters
      </button>
      <button type="button" onClick={() => ctx.setFilters({ search: 'beta' })}>
        apply-search-beta
      </button>
      <button type="button" onClick={() => ctx.setFilters({ search: '  ' })}>
        apply-search-blank
      </button>
      <button type="button" onClick={() => ctx.setFilters({ search: '  beta  ' })}>
        apply-search-padded
      </button>
      <button
        type="button"
        onClick={() => ctx.setFilters({ sinceMs: 1_000, outputFolder: '/out', sort: 'newest' })}
      >
        apply-range
      </button>
      <button type="button" onClick={() => ctx.setFilters({ statuses: ['failed'] })}>
        apply-statuses-only
      </button>
      <button type="button" onClick={() => ctx.setPage(2)}>
        page-two
      </button>
      <button type="button" onClick={() => void ctx.clearHistory()}>
        clear-history
      </button>
      <button type="button" onClick={() => ctx.setExpanded(false)}>
        collapse
      </button>
      <button type="button" onClick={ctx.refresh}>
        refresh
      </button>
      <button type="button" onClick={ctx.clearError}>
        clear-error
      </button>
    </div>
  )
}

const renderProvider = async () => {
  render(
    <HistoryProvider>
      <Probe />
    </HistoryProvider>
  )
  await flush()
  await advance(300)
}

beforeEach(() => {
  vi.clearAllMocks()
  registry.clear()
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('HistoryProvider', () => {
  it('selects the first session after load and starts expanded', async () => {
    mockHistory()
    await renderProvider()
    expect(historyCalls()).toHaveLength(1)
    expect(screen.getByTestId('selected')).toHaveTextContent('s1')
    expect(screen.getByTestId('expanded')).toHaveTextContent('true')
    expect(screen.getByTestId('page')).toHaveTextContent('0')
  })

  it('keeps the selection when it survives a refresh or batch completion', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('select-second'))
    expect(screen.getByTestId('selected')).toHaveTextContent('s2')

    fireEvent.click(screen.getByText('refresh'))
    await flush()
    expect(screen.getByTestId('selected')).toHaveTextContent('s2')

    act(() => {
      registry.get('batch:complete')?.forEach((handler) => handler({ payload: {} }))
    })
    await flush()
    expect(screen.getByTestId('selected')).toHaveTextContent('s2')
  })

  it('ignores a selectSession id that is not in the response', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('select-missing'))
    expect(screen.getByTestId('selected')).toHaveTextContent('s1')
  })

  it('falls back to the first session when the selected one is filtered out', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('select-second'))
    expect(screen.getByTestId('selected')).toHaveTextContent('s2')

    fireEvent.click(screen.getByText('type-alpha'))
    await advance(300)
    await flush()
    expect(screen.getByTestId('selected')).toHaveTextContent('s1')
  })

  it('clears the selection when no session matches', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('type-missing'))
    await advance(300)
    await flush()
    expect(screen.getByTestId('selected')).toHaveTextContent('none')
  })

  it('debounces the trimmed search by 300 ms', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('find-alpha'))
    await advance(299)
    expect(searchCalls()).toHaveLength(0)
    await advance(1)
    expect(searchCalls()).toHaveLength(1)
    expect(searchCalls()[0][1]).toMatchObject({ filters: { search: 'alpha' }, page: 0 })
    await flush()
  })

  it('restarts the debounce on each change and sends the latest text once', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('type-a'))
    await advance(100)
    fireEvent.click(screen.getByText('type-alpha'))
    await advance(200)
    expect(searchCalls()).toHaveLength(0)
    await advance(100)
    expect(searchCalls()).toHaveLength(1)
    expect(lastFilters().search).toBe('alpha')
    await flush()
  })

  it('cancels a pending search when the text is cleared without refetching', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('type-alpha'))
    await advance(100)
    const before = historyCalls().length
    fireEvent.click(screen.getByText('clear-search'))
    await advance(300)
    await flush()
    expect(historyCalls()).toHaveLength(before)
    expect(lastFilters().search).toBeUndefined()
  })

  it('refetches once with search undefined when an applied search is cleared', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('type-alpha'))
    await advance(300)
    await flush()
    expect(lastFilters().search).toBe('alpha')

    const before = historyCalls().length
    fireEvent.click(screen.getByText('clear-search'))
    await advance(300)
    await flush()
    expect(historyCalls()).toHaveLength(before + 1)
    expect(lastFilters().search).toBeUndefined()
  })

  it('merges a debounced search with filters applied in the meantime', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('type-alpha'))
    await advance(100)
    fireEvent.click(screen.getByText('apply-category'))
    await advance(200)
    await flush()
    expect(lastFilters().search).toBe('alpha')
    expect(lastFilters().categories).toEqual(['image'])
  })

  it('keeps the applied search when filters are replaced without a search key', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('type-alpha'))
    await advance(300)
    await flush()
    expect(lastFilters().search).toBe('alpha')

    fireEvent.click(screen.getByText('apply-category'))
    await flush()
    expect(lastFilters().search).toBe('alpha')
    expect(lastFilters().categories).toEqual(['image'])
    expect(screen.getByTestId('search-text')).toHaveTextContent('alpha')
  })

  it('normalizes a blank search to undefined', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('apply-search-blank'))
    await flush()
    expect(displayedFilters().search).toBeUndefined()
    expect(screen.getByTestId('search-text').textContent).toBe('')
  })

  it('trims a padded search supplied through setFilters', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('apply-search-padded'))
    await flush()
    expect(displayedFilters().search).toBe('beta')
    expect(screen.getByTestId('search-text').textContent).toBe('beta')
    expect(lastFilters().search).toBe('beta')
  })

  it('replaces filters so clearing statuses and categories sticks', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('apply-status'))
    await flush()
    expect(lastFilters().statuses).toEqual(['failed'])
    expect(lastFilters().categories).toEqual(['image'])

    fireEvent.click(screen.getByText('clear-filters'))
    await flush()
    expect(lastFilters().sort).toBe('newest')
    expect(lastFilters().statuses).toBeUndefined()
    expect(lastFilters().categories).toBeUndefined()
  })

  it('clears every omitted filter key on replacement', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('apply-range'))
    await flush()
    expect(lastFilters().sinceMs).toBe(1_000)
    expect(lastFilters().outputFolder).toBe('/out')
    expect(lastFilters().sort).toBe('newest')

    fireEvent.click(screen.getByText('apply-statuses-only'))
    await flush()
    expect(lastFilters().statuses).toEqual(['failed'])
    expect(lastFilters().sinceMs).toBeUndefined()
    expect(lastFilters().outputFolder).toBeUndefined()
    expect(lastFilters().sort).toBeUndefined()
    expect(lastFilters().categories).toBeUndefined()
  })

  it('syncs searchText and cancels the debounce when setFilters includes search', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('type-alpha'))
    await advance(100)
    fireEvent.click(screen.getByText('apply-search-beta'))
    await flush()
    expect(screen.getByTestId('search-text')).toHaveTextContent('beta')
    expect(lastFilters().search).toBe('beta')

    await advance(300)
    await flush()
    expect(
      searchCalls().some(
        ([, args]) => (args as { filters: HistoryFilters }).filters.search === 'alpha'
      )
    ).toBe(false)
    expect(lastFilters().search).toBe('beta')
  })

  it('exposes loading, filters and page, and refresh refetches', async () => {
    mockHistory()
    await renderProvider()
    const before = historyCalls().length

    fireEvent.click(screen.getByText('refresh'))
    expect(screen.getByTestId('loading')).toHaveTextContent('true')
    await flush()
    expect(screen.getByTestId('loading')).toHaveTextContent('false')
    expect(historyCalls().length).toBe(before + 1)
    expect(screen.getByTestId('page')).toHaveTextContent('0')
    expect(JSON.parse(screen.getByTestId('filters').textContent ?? '{}')).toEqual(lastFilters())
  })

  it('clears a load error through clearError', async () => {
    vi.mocked(invoke).mockRejectedValue(new Error('boom'))
    await renderProvider()
    expect(screen.getByTestId('error')).toHaveTextContent('Could not load history: boom')
    fireEvent.click(screen.getByText('clear-error'))
    expect(screen.getByTestId('error')).toHaveTextContent('none')
    await flush()
    expect(screen.getByTestId('error')).toHaveTextContent('none')

    fireEvent.click(screen.getByText('refresh'))
    await flush()
    expect(screen.getByTestId('error')).toHaveTextContent('Could not load history: boom')
  })

  it('pages and resets the page when filters change', async () => {
    mockHistory()
    await renderProvider()
    fireEvent.click(screen.getByText('page-two'))
    await flush()
    const paged = historyCalls()[historyCalls().length - 1][1] as { page: number }
    expect(paged.page).toBe(2)
    expect(screen.getByTestId('page')).toHaveTextContent('2')

    fireEvent.click(screen.getByText('apply-category'))
    await flush()
    const filtered = historyCalls()[historyCalls().length - 1][1] as { page: number }
    expect(filtered.page).toBe(0)
    expect(screen.getByTestId('page')).toHaveTextContent('0')
  })

  it('clears history and refetches', async () => {
    mockHistory()
    await renderProvider()
    const before = historyCalls().length
    fireEvent.click(screen.getByText('clear-history'))
    await flush()
    expect(invoke).toHaveBeenCalledWith('clear_history')
    expect(historyCalls()).toHaveLength(before + 1)
  })

  it('updates expanded', async () => {
    mockHistory()
    await renderProvider()
    expect(screen.getByTestId('expanded')).toHaveTextContent('true')
    fireEvent.click(screen.getByText('collapse'))
    expect(screen.getByTestId('expanded')).toHaveTextContent('false')
  })

  it('throws when used outside the provider', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    expect(() => render(<Probe />)).toThrow(
      'useHistoryContext must be used within HistoryProvider'
    )
    spy.mockRestore()
  })
})
