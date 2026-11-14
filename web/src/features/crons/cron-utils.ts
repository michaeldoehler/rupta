import type { Monitor } from '@/api/types'

const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']

/** Plain-English description of common crontab and interval schedules. */
export function describeSchedule(s: Monitor['schedule']): string {
  if (s.type === 'interval') {
    const n = Number(s.value)
    const unit = s.unit ?? 'minute'
    return n === 1 ? `Every ${unit}` : `Every ${n} ${unit}s`
  }
  const parts = String(s.value).trim().split(/\s+/)
  if (parts.length !== 5) return String(s.value)
  const [min, hour, dom, mon, dow] = parts
  const time = /^\d+$/.test(min) && /^\d+$/.test(hour) ? `${hour.padStart(2, '0')}:${min.padStart(2, '0')}` : null
  if (min.startsWith('*/') && hour === '*' && dom === '*' && mon === '*' && dow === '*') return `Every ${min.slice(2)} minutes`
  if (/^\d+$/.test(min) && hour === '*' && dom === '*' && mon === '*' && dow === '*') return `Every hour at :${min.padStart(2, '0')}`
  if (time && dom === '*' && mon === '*' && dow === '*') return `Every day at ${time}`
  if (time && dom === '*' && mon === '*' && dow === '1-5') return `Weekdays at ${time}`
  if (time && dom === '*' && mon === '*' && /^\d$/.test(dow)) return `Every ${DAYS[Number(dow) % 7]} at ${time}`
  if (time && /^\d+$/.test(dom) && mon === '*' && dow === '*') return `Monthly on day ${dom} at ${time}`
  return String(s.value)
}
