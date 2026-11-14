import { useEffect, useRef, useState } from 'react'
import { normLog } from '@/api/telemetry'
import type { LogRow } from '@/api/telemetry-types'

export type TailStatus = 'off' | 'connecting' | 'live' | 'reconnecting' | 'error'

/** Parses Server-Sent Events from text chunks: returns complete events and the unparsed rest. */
export function parseSse(buffer: string): { events: { event: string; data: string }[]; rest: string } {
  const events: { event: string; data: string }[] = []
  const blocks = buffer.split(/\r?\n\r?\n/)
  const rest = blocks.pop() ?? ''
  for (const block of blocks) {
    let event = 'message'
    const data: string[] = []
    for (const line of block.split(/\r?\n/)) {
      if (line.startsWith(':')) continue
      const i = line.indexOf(':')
      const field = i === -1 ? line : line.slice(0, i)
      const value = i === -1 ? '' : line.slice(i + 1).replace(/^ /, '')
      if (field === 'event') event = value
      else if (field === 'data') data.push(value)
    }
    if (data.length) events.push({ event, data: data.join('\n') })
  }
  return { events, rest }
}

export interface TailState {
  status: TailStatus
  /** Records the server skipped because too many arrived at once (`lagging`). */
  skipped: number
  /** Last error the server reported on the stream (`error` event). */
  message: string | null
}

/**
 * Live tail over Server-Sent Events (`GET /logs/stream`). Uses fetch with a
 * stream reader rather than EventSource, so it sends the session cookie the
 * same way as every other request and stops cleanly. Reconnects with a
 * growing delay. `onRows` receives new rows, newest last.
 */
export function useLiveTail(url: string | null, onRows: (rows: LogRow[]) => void): TailState {
  const [status, setStatus] = useState<TailStatus>('off')
  const [skipped, setSkipped] = useState(0)
  const [message, setMessage] = useState<string | null>(null)
  const cb = useRef(onRows)
  useEffect(() => {
    cb.current = onRows
  })
  useEffect(() => {
    setSkipped(0)
    setMessage(null)
    if (!url) {
      setStatus('off')
      return
    }
    const ctrl = new AbortController()
    let attempt = 0
    let seq = 0
    let timer: ReturnType<typeof setTimeout> | undefined
    const connect = async () => {
      setStatus(attempt ? 'reconnecting' : 'connecting')
      try {
        const res = await fetch(url, { headers: { Accept: 'text/event-stream' }, credentials: 'same-origin', signal: ctrl.signal })
        if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`)
        setStatus('live')
        attempt = 0
        const reader = res.body.getReader()
        const dec = new TextDecoder()
        let buf = ''
        for (;;) {
          const { done, value } = await reader.read()
          if (done) break
          buf += dec.decode(value, { stream: true })
          const { events, rest } = parseSse(buf)
          buf = rest
          const rows: LogRow[] = []
          for (const ev of events) {
            if (ev.event === 'ready') {
              setMessage(null)
              continue
            }
            if (ev.event === 'lagging' || ev.event === 'error') {
              try {
                const d = JSON.parse(ev.data) as { skipped?: number; message?: string }
                if (ev.event === 'lagging') setSkipped((n) => n + Number(d.skipped ?? 0))
                else setMessage(d.message ?? 'The server could not read new log lines.')
              } catch {
                /* ignore */
              }
              continue
            }
            if (ev.event !== 'message' && ev.event !== 'log' && ev.event !== 'logs') continue
            setMessage(null)
            try {
              const parsed: unknown = JSON.parse(ev.data)
              const list = Array.isArray(parsed) ? parsed : [parsed]
              for (const item of list) {
                const row = normLog(item, seq++)
                if (!(item as { id?: unknown }).id) row.id = `live-${seq}-${row.timestamp}`
                rows.push(row)
              }
            } catch {
              /* ignore malformed events */
            }
          }
          if (rows.length) cb.current(rows)
        }
        if (!ctrl.signal.aborted) throw new Error('stream ended')
      } catch {
        if (ctrl.signal.aborted) return
        attempt += 1
        setStatus(attempt > 5 ? 'error' : 'reconnecting')
        timer = setTimeout(connect, Math.min(15_000, 1000 * 2 ** Math.min(attempt, 4)))
      }
    }
    void connect()
    return () => {
      ctrl.abort()
      clearTimeout(timer)
    }
  }, [url])
  return { status, skipped, message }
}
