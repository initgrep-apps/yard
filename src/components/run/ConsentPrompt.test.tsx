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

import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { ConsentPrompt } from '@/components/run/ConsentPrompt'

const request = {
  requestId: 'r1',
  fileId: 'f1',
  category: 'image' as const,
  changes: ['downscale'] as const,
  detail: 'This file needs permission to downscale',
}

describe('ConsentPrompt', () => {
  it('renders an inline prompt, never a modal', () => {
    const onAnswer = vi.fn()
    render(<ConsentPrompt request={{ ...request, changes: [...request.changes] }} onAnswer={onAnswer} />)

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(screen.getByTestId('consent-prompt')).toBeInTheDocument()
    expect(screen.getByText('This file needs permission to downscale')).toBeInTheDocument()
    expect(screen.getByText('Downscale')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Allow' }))
    expect(onAnswer).toHaveBeenCalledWith('r1', true, 'once')

    fireEvent.click(screen.getByRole('button', { name: 'Skip' }))
    expect(onAnswer).toHaveBeenCalledWith('r1', false, 'once')
  })

  it('apply to the rest of this batch answers with batch scope for allow and skip', () => {
    const onAnswer = vi.fn()
    render(<ConsentPrompt request={{ ...request, changes: [...request.changes] }} onAnswer={onAnswer} />)

    fireEvent.click(screen.getByRole('checkbox', { name: 'Apply to the rest of this batch' }))
    fireEvent.click(screen.getByRole('button', { name: 'Allow' }))

    expect(onAnswer).toHaveBeenCalledWith('r1', true, 'batch')

    fireEvent.click(screen.getByRole('button', { name: 'Skip' }))
    expect(onAnswer).toHaveBeenLastCalledWith('r1', false, 'batch')
  })
})
