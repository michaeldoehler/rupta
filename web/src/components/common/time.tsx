import { useEffect, useState } from 'react'
import { Tip } from '@/components/ui/tooltip'
import { absolute, relLong, relShort } from '@/lib/format'
import { cn } from '@/lib/utils'

// One shared ticker re-renders relative times every 30 s.
const listeners = new Set<() => void>()
let timer: ReturnType<typeof setInterval> | undefined
function subscribe(fn: () => void) {
  listeners.add(fn)
  if (!timer) timer = setInterval(() => listeners.forEach((l) => l()), 30_000)
  return () => {
    listeners.delete(fn)
    if (listeners.size === 0 && timer) {
      clearInterval(timer)
      timer = undefined
    }
  }
}

function useTick() {
  const [, set] = useState(0)
  useEffect(() => subscribe(() => set((n) => n + 1)), [])
}

export function RelativeTime({
  date,
  long = false,
  className,
  suffix = false,
}: {
  date: string | number | Date
  long?: boolean
  className?: string
  suffix?: boolean
}) {
  useTick()
  const d = new Date(date)
  const short = relShort(d)
  const text = long ? relLong(d) : `${short}${suffix && short !== 'now' && !short.startsWith('in ') ? ' ago' : ''}`
  return (
    <Tip content={absolute(d, { seconds: true })}>
      <time dateTime={d.toISOString()} className={cn('whitespace-nowrap tabular', className)}>
        {text}
      </time>
    </Tip>
  )
}
