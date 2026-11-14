import { useState } from 'react'
import { cn } from '@/lib/utils'

/** Renders any JSON-ish value compactly; objects expand on click. */
export function ValueView({ value, className }: { value: unknown; className?: string }) {
  const [open, setOpen] = useState(false)
  if (value === null || value === undefined) return <span className={cn('font-mono text-muted-foreground italic', className)}>{String(value)}</span>
  if (typeof value === 'string') {
    const filtered = value === '[Filtered]'
    return (
      <span className={cn('font-mono break-all whitespace-pre-wrap', filtered ? 'text-muted-foreground italic' : 'text-foreground', className)}>
        {filtered ? value : `"${value}"`}
      </span>
    )
  }
  if (typeof value === 'number' || typeof value === 'boolean') return <span className={cn('font-mono text-brand-text', className)}>{String(value)}</span>
  const json = JSON.stringify(value, null, 2)
  const short = JSON.stringify(value)
  if (short.length <= 80) return <span className={cn('font-mono break-all', className)}>{short}</span>
  return (
    <div className={cn('font-mono', className)}>
      <button type="button" className="text-left text-muted-foreground hover:text-foreground" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        {open ? '▾' : '▸'} {Array.isArray(value) ? `Array(${value.length})` : `{${Object.keys(value as object).length} keys}`}
      </button>
      {open ? (
        <pre className="mt-1 overflow-x-auto text-[12px] leading-relaxed whitespace-pre-wrap">{json}</pre>
      ) : (
        <span className="ml-2 break-all text-muted-foreground">{short.slice(0, 80)}…</span>
      )}
    </div>
  )
}
