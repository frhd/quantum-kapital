"use client"

import { useState } from "react"
import { CalendarIcon } from "lucide-react"

import { Button } from "./ui/button"
import { Calendar } from "./ui/calendar"
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover"
import { cn } from "../lib/utils"

const ET_DATE_FMT = new Intl.DateTimeFormat("en-CA", { timeZone: "America/New_York" })

function todayEt(): string {
  return ET_DATE_FMT.format(new Date())
}

function isoToDate(iso: string): Date | undefined {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso)
  if (!m) return undefined
  return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
}

function dateToIso(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, "0")
  const day = String(d.getDate()).padStart(2, "0")
  return `${y}-${m}-${day}`
}

export interface DatePickerProps {
  value: string
  onChange: (iso: string) => void
  ariaLabel?: string
  className?: string
}

export function DatePicker({ value, onChange, ariaLabel, className }: DatePickerProps) {
  const [open, setOpen] = useState(false)
  const selected = isoToDate(value)

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          aria-label={ariaLabel}
          className={cn("h-8 gap-2 px-2 text-xs font-normal", className)}
        >
          <CalendarIcon className="h-3.5 w-3.5" />
          <span>{value}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-auto p-2">
        <Calendar
          mode="single"
          selected={selected}
          onSelect={(d) => {
            if (d) {
              onChange(dateToIso(d))
              setOpen(false)
            }
          }}
        />
        <div className="border-border mt-2 flex items-center justify-end gap-2 border-t pt-2">
          <Button
            size="sm"
            variant="ghost"
            className="h-7 text-xs"
            onClick={() => {
              onChange(todayEt())
              setOpen(false)
            }}
          >
            Today
          </Button>
          <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={() => setOpen(false)}>
            Cancel
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}
