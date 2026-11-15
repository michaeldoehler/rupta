import { CornerDownLeft, Search, X } from 'lucide-react'
import { forwardRef, useDeferredValue, useEffect, useId, useImperativeHandle, useMemo, useRef, useState } from 'react'
import { useTagValues } from '@/api/queries'
import { Kbd } from '@/components/ui/kbd'
import {
  applySuggestion,
  completionContext,
  dynamicTagFor,
  suggestKeys,
  suggestValues,
  type CompletionContext,
  type Suggestion,
} from '@/lib/search/autocomplete'
import { tokenize } from '@/lib/search/parse'
import { cn } from '@/lib/utils'

export interface SearchBarHandle {
  focus: () => void
}

interface Props {
  org: string
  projects: string[]
  value: string
  onSubmit: (query: string) => void
  placeholder?: string
  className?: string
  /** Accessible name of the input (default "Search issues"). */
  label?: string
  /** Suggestions for another query language (logs, replays); default: issue keys and tag values. */
  suggest?: (ctx: CompletionContext) => Suggestion[]
}

/** Renders the query with coloured tokens, behind the transparent input text. */
function Highlight({ value }: { value: string }) {
  const parts: React.ReactNode[] = []
  let pos = 0
  for (const t of tokenize(value)) {
    if (t.start > pos) parts.push(value.slice(pos, t.start))
    if (t.type === 'filter') {
      const keyEnd = t.valueStart
      const negated = t.raw.startsWith('!')
      parts.push(
        <span key={t.start} className="rounded-[3px] bg-brand-soft/70 shadow-[0_0_0_1px_var(--brand-soft)]">
          {negated && <span className="text-destructive">!</span>}
          <span className="text-brand-text">{value.slice(t.start + (negated ? 1 : 0), keyEnd)}</span>
          <span className="text-foreground">{value.slice(keyEnd, t.end)}</span>
        </span>,
      )
    } else {
      parts.push(
        <span key={t.start} className="text-foreground">
          {t.raw}
        </span>,
      )
    }
    pos = t.end
  }
  if (pos < value.length) parts.push(value.slice(pos))
  return <>{parts}</>
}

export const SearchBar = forwardRef<SearchBarHandle, Props>(function SearchBar(
  {
    org,
    projects,
    value,
    onSubmit,
    placeholder = 'Search issues: is:unresolved level:error browser:Chrome "text"',
    className,
    label = 'Search issues',
    suggest,
  },
  ref,
) {
  const inputRef = useRef<HTMLInputElement>(null)
  const overlayRef = useRef<HTMLDivElement>(null)
  const [draft, setDraft] = useState(value)
  const [caret, setCaret] = useState(value.length)
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(0)
  const listId = useId()

  useImperativeHandle(ref, () => ({ focus: () => inputRef.current?.focus() }))
  useEffect(() => setDraft(value), [value])

  const ctx = useMemo(() => completionContext(draft, caret), [draft, caret])
  const dynKey = ctx.mode === 'value' && ctx.key ? dynamicTagFor(ctx.key) : null
  const partial = useDeferredValue(ctx.partial)
  const tagValues = useTagValues(org, open && !suggest ? dynKey : null, projects, partial)

  const suggestions: Suggestion[] = useMemo(() => {
    if (suggest) return suggest(ctx)
    if (ctx.mode === 'key') return suggestKeys(ctx.partial)
    return suggestValues(ctx.key!, ctx.partial, tagValues.data?.map((v) => v.value) ?? [])
  }, [ctx, tagValues.data, suggest])

  useEffect(() => setActive(0), [ctx.mode, ctx.key, ctx.partial])

  function syncCaret() {
    const el = inputRef.current
    if (el) setCaret(el.selectionStart ?? el.value.length)
    if (el && overlayRef.current) overlayRef.current.scrollLeft = el.scrollLeft
  }

  function apply(s: Suggestion) {
    const next = applySuggestion(draft, ctx, s)
    setDraft(next.query)
    setCaret(next.cursor)
    requestAnimationFrame(() => {
      const el = inputRef.current
      if (!el) return
      el.focus()
      el.setSelectionRange(next.cursor, next.cursor)
      if (overlayRef.current) overlayRef.current.scrollLeft = el.scrollLeft
    })
    if (s.kind === 'value') setOpen(true)
  }

  function submit(q = draft) {
    setOpen(false)
    onSubmit(q.trim().replace(/\s+/g, ' '))
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    const showing = open && suggestions.length > 0
    if (e.key === 'ArrowDown' && showing) {
      e.preventDefault()
      setActive((a) => (a + 1) % suggestions.length)
    } else if (e.key === 'ArrowUp' && showing) {
      e.preventDefault()
      setActive((a) => (a - 1 + suggestions.length) % suggestions.length)
    } else if (e.key === 'Tab' && showing && (ctx.partial || ctx.mode === 'value')) {
      e.preventDefault()
      apply(suggestions[active])
    } else if (e.key === 'Enter') {
      e.preventDefault()
      // Enter applies a suggestion only while a key/value is being typed.
      if (showing && (ctx.partial || ctx.mode === 'value') && !(ctx.mode === 'key' && suggestions[active]?.label === ctx.partial)) apply(suggestions[active])
      else submit()
    } else if (e.key === 'Escape') {
      if (open) {
        e.stopPropagation()
        setOpen(false)
      } else inputRef.current?.blur()
    }
  }

  const showList = open && suggestions.length > 0
  return (
    <div className={cn('relative', className)}>
      <div
        className="group relative flex h-9 items-center rounded-md border border-input bg-card shadow-xs transition-[box-shadow,border-color] focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/25"
        onClick={() => inputRef.current?.focus()}
      >
        <Search className="pointer-events-none ml-2.5 size-4 shrink-0 text-muted-foreground" />
        <div className="relative h-full min-w-0 flex-1">
          <div
            ref={overlayRef}
            aria-hidden
            className="pointer-events-none absolute inset-0 overflow-hidden px-2 font-mono text-[13px] leading-[34px] whitespace-pre text-transparent"
          >
            <Highlight value={draft} />
          </div>
          <input
            ref={inputRef}
            role="combobox"
            aria-expanded={showList}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={showList ? `${listId}-${active}` : undefined}
            aria-label={label}
            spellCheck={false}
            autoComplete="off"
            className="relative h-full w-full bg-transparent px-2 font-mono text-[13px] text-ellipsis text-transparent caret-foreground outline-none selection:bg-brand/25 selection:text-foreground placeholder:font-sans placeholder:text-muted-foreground"
            placeholder={placeholder}
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value)
              setOpen(true)
              syncCaret()
            }}
            onKeyDown={onKeyDown}
            onKeyUp={syncCaret}
            onClick={() => {
              setOpen(true)
              syncCaret()
            }}
            onScroll={syncCaret}
            onFocus={() => {
              setOpen(true)
              syncCaret()
            }}
            onBlur={() => setTimeout(() => setOpen(false), 120)}
          />
        </div>
        {draft && (
          <button
            type="button"
            className="mr-1 flex size-6 items-center justify-center rounded text-muted-foreground hover:text-foreground"
            aria-label="Clear search"
            onClick={(e) => {
              e.stopPropagation()
              setDraft('')
              submit('')
            }}
          >
            <X className="size-3.5" />
          </button>
        )}
        <Kbd className="mr-2 hidden group-focus-within:hidden sm:inline-flex">/</Kbd>
      </div>
      {showList && (
        <div className="absolute top-full right-0 left-0 z-40 mt-1 overflow-hidden rounded-lg border bg-popover shadow-lg">
          <div className="flex items-center justify-between border-b px-3 py-1.5 text-[11px] text-muted-foreground">
            <span>{ctx.mode === 'key' ? 'Filter keys' : `Values for ${ctx.key}`}</span>
            <span className="hidden items-center gap-1 sm:flex">
              <Kbd>↑</Kbd>
              <Kbd>↓</Kbd> to move, <Kbd>Tab</Kbd> to complete, <Kbd>↵</Kbd> to search
            </span>
          </div>
          <ul id={listId} role="listbox" aria-label="Suggestions" className="max-h-80 overflow-y-auto p-1">
            {suggestions.map((s, i) => (
              <li
                key={`${s.kind}-${s.label}`}
                id={`${listId}-${i}`}
                role="option"
                aria-selected={i === active}
                className={cn('flex cursor-default items-center gap-3 rounded-md px-2 py-1.5 text-[13px]', i === active && 'bg-accent')}
                onMouseDown={(e) => {
                  e.preventDefault()
                  apply(s)
                }}
                onMouseEnter={() => setActive(i)}
              >
                <span className="min-w-0 truncate font-mono">
                  {s.kind === 'key' ? (
                    <>
                      <span className="text-brand-text">{s.label}</span>
                      <span className="text-muted-foreground">:</span>
                    </>
                  ) : (
                    s.label
                  )}
                </span>
                {s.description && <span className="min-w-0 truncate text-xs text-muted-foreground">{s.description}</span>}
                {i === active && <CornerDownLeft className="ml-auto size-3.5 shrink-0 text-muted-foreground" />}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
})
