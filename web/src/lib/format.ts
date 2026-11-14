import { differenceInSeconds, format as fmt, formatDistanceToNowStrict } from 'date-fns'

let preferredTimezone: string | null = null
let clock24 = true

/** Set from user preferences; `null` = the browser's timezone. */
export function setTimePreferences(tz: string | null | undefined, use24h: boolean | undefined) {
  preferredTimezone = tz && tz !== 'browser' ? tz : null
  clock24 = use24h ?? true
}

function toDate(v: string | number | Date): Date {
  return v instanceof Date ? v : new Date(v)
}

/** Compact relative time: "now", "42s", "5m", "3h", "2d", "4mo", "1y". */
export function relShort(v: string | number | Date, now: Date = new Date()): string {
  const diff = differenceInSeconds(now, toDate(v))
  if (diff < -10) return `in ${relShort(now, new Date(now.getTime() - diff * 1000))}`
  const s = Math.max(0, diff)
  if (s < 10) return 'now'
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h`
  const d = Math.floor(h / 24)
  if (d < 30) return `${d}d`
  const mo = Math.floor(d / 30)
  if (mo < 12) return `${mo}mo`
  return `${Math.floor(d / 365)}y`
}

export function relLong(v: string | number | Date): string {
  const d = toDate(v)
  if (Math.abs(Date.now() - d.getTime()) < 10_000) return 'just now'
  return d.getTime() > Date.now() ? `in ${formatDistanceToNowStrict(d)}` : `${formatDistanceToNowStrict(d)} ago`
}

/** Absolute date/time in the preferred timezone. */
export function absolute(v: string | number | Date, opts: { seconds?: boolean; date?: boolean; ms?: boolean } = {}): string {
  const d = toDate(v)
  const withDate = opts.date ?? true
  try {
    const parts: Intl.DateTimeFormatOptions = {
      hour: '2-digit',
      minute: '2-digit',
      second: opts.seconds || opts.ms ? '2-digit' : undefined,
      fractionalSecondDigits: opts.ms ? 3 : undefined,
      hour12: !clock24,
      timeZone: preferredTimezone ?? undefined,
    }
    if (withDate) Object.assign(parts, { year: 'numeric', month: 'short', day: 'numeric' })
    return new Intl.DateTimeFormat('en-US', parts).format(d)
  } catch {
    return fmt(d, withDate ? 'MMM d, yyyy HH:mm' : 'HH:mm:ss')
  }
}

export function isoDay(v: string | number | Date): string {
  return fmt(toDate(v), 'yyyy-MM-dd')
}

/** 1234 -> "1.2k", 1234567 -> "1.2M". */
export function compact(n: number | null | undefined): string {
  if (n === null || n === undefined || Number.isNaN(n)) return '—'
  if (Math.abs(n) < 1000) return String(n)
  if (Math.abs(n) < 1_000_000) return `${trim(n / 1000)}k`
  if (Math.abs(n) < 1_000_000_000) return `${trim(n / 1_000_000)}M`
  return `${trim(n / 1_000_000_000)}B`
}

function trim(x: number): string {
  return x >= 100 ? Math.round(x).toString() : x.toFixed(1).replace(/\.0$/, '')
}

export function thousands(n: number | null | undefined): string {
  if (n === null || n === undefined) return '—'
  return n.toLocaleString('en-US')
}

export function percent(n: number | null | undefined, digits = 2): string {
  if (n === null || n === undefined) return '—'
  const fixed = n.toFixed(digits)
  return `${fixed.includes('.') ? fixed.replace(/0+$/, '').replace(/\.$/, '') : fixed}%`
}

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`
  const units = ['KiB', 'MiB', 'GiB', 'TiB']
  let v = n / 1024
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i += 1
  }
  return `${v.toFixed(v >= 10 ? 0 : 1)} ${units[i]}`
}

export function duration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return '—'
  if (ms < 1000) return `${Math.round(ms)} ms`
  const s = ms / 1000
  if (s < 60) return `${s.toFixed(s < 10 ? 2 : 1)} s`
  const m = Math.floor(s / 60)
  const rs = Math.round(s % 60)
  if (m < 60) return `${m}m ${rs}s`
  return `${Math.floor(m / 60)}h ${m % 60}m`
}

/** Shortens a release like "web@1.4.2+abc123def" to "1.4.2 (abc123d)". */
export function shortVersion(v: string): string {
  const noPkg = v.includes('@') ? v.slice(v.lastIndexOf('@') + 1) : v
  if (/^[0-9a-f]{12,40}$/.test(noPkg)) return noPkg.slice(0, 7)
  const [base, build] = noPkg.split('+')
  return build ? `${base} (${build.slice(0, 7)})` : base
}
