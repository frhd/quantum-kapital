"use client"

import * as React from "react"
import { DayPicker } from "react-day-picker"
import "react-day-picker/style.css"

import { cn } from "../../lib/utils"

export type CalendarProps = React.ComponentProps<typeof DayPicker>

export function Calendar({
  className,
  classNames,
  showOutsideDays = true,
  ...props
}: CalendarProps) {
  return (
    <DayPicker
      showOutsideDays={showOutsideDays}
      className={cn("text-sm", className)}
      classNames={{
        today: "font-semibold underline",
        selected: "bg-primary text-primary-foreground rounded-md",
        ...classNames,
      }}
      {...props}
    />
  )
}
