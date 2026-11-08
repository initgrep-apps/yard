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

import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { UpdateToast } from '@/components/update/UpdateToast'

describe('UpdateToast', () => {
  it('shows download version, percentage, and bytes', () => {
    render(
      <UpdateToast
        status="downloading"
        version="1.2.0"
        percent={42}
        downloaded={13_002_342}
        total={100_873_011}
        isCompressing={false}
      />,
    )
    expect(screen.getByText('Downloading update')).toBeInTheDocument()
    expect(screen.getByText('Version 1.2.0 · 42%')).toBeInTheDocument()
    expect(screen.getByText('12.4 MB of 96.2 MB')).toBeInTheDocument()
  })

  it('shows ready copy without the app name', () => {
    const { container } = render(
      <UpdateToast
        status="ready"
        version="1.2.0"
        percent={100}
        downloaded={1}
        total={1}
        isCompressing={false}
      />,
    )
    expect(screen.getByText('Update ready')).toBeInTheDocument()
    expect(screen.getByText('Restart the app to finish.')).toBeInTheDocument()
    expect(container.textContent).not.toMatch(/Yard/)
  })

  it('explains the batch when ready while compressing', () => {
    render(
      <UpdateToast
        status="ready"
        version="1.2.0"
        percent={100}
        downloaded={1}
        total={1}
        isCompressing
      />,
    )
    expect(screen.getByText('Finish the current batch to restart.')).toBeInTheDocument()
    expect(screen.queryByText('Restart the app to finish.')).not.toBeInTheDocument()
  })

  it('shows installing copy', () => {
    render(
      <UpdateToast
        status="installing"
        version="1.2.0"
        percent={100}
        downloaded={1}
        total={1}
        isCompressing={false}
      />,
    )
    expect(screen.getByText('Installing update')).toBeInTheDocument()
    expect(screen.getByText('The app will restart.')).toBeInTheDocument()
  })

  it('shows an indeterminate bar when the total size is unknown', () => {
    render(
      <UpdateToast
        status="downloading"
        version="1.2.0"
        percent={null}
        downloaded={13_002_342}
        total={null}
        isCompressing={false}
      />,
    )
    expect(screen.getByText('Version 1.2.0')).toBeInTheDocument()
    expect(screen.getByText('12.4 MB')).toBeInTheDocument()
    expect(screen.getByRole('progressbar')).toBeInTheDocument()
  })
})
