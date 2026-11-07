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

import { NumberField } from '@base-ui/react/number-field'
import { Minus, Plus } from 'lucide-react'
import { cn } from '@/lib/utils'

interface NumberStepperProps {
  value: number
  onValueChange: (value: number) => void
  min: number
  max?: number
  ariaLabel: string
  id?: string
  className?: string
  disabled?: boolean
}

export function NumberStepper({
  value,
  onValueChange,
  min,
  max,
  ariaLabel,
  id,
  className,
  disabled,
}: NumberStepperProps) {
  return (
    <NumberField.Root
      disabled={disabled}
      value={value}
      onValueChange={(next) => {
        if (next === null || !Number.isFinite(next)) return
        onValueChange(Math.round(next))
      }}
      min={min}
      max={max}
      step={1}
      className={cn('w-32 data-[disabled]:opacity-50', className)}
    >
      <NumberField.Group className="flex h-9 items-center overflow-hidden rounded-md border border-input bg-background/50 shadow-xs outline-none transition-[color,box-shadow] focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50">
        <NumberField.Decrement
          aria-label={`Decrease ${ariaLabel}`}
          className="flex h-full w-8 shrink-0 items-center justify-center rounded-l-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
        >
          <Minus data-icon="inline-start" />
        </NumberField.Decrement>
        <NumberField.Input
          id={id}
          aria-label={ariaLabel}
          className="h-full w-full min-w-0 border-x border-input bg-transparent px-2 text-right text-sm tabular-nums outline-none"
        />
        <NumberField.Increment
          aria-label={`Increase ${ariaLabel}`}
          className="flex h-full w-8 shrink-0 items-center justify-center rounded-r-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
        >
          <Plus data-icon="inline-start" />
        </NumberField.Increment>
      </NumberField.Group>
    </NumberField.Root>
  )
}
