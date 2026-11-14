import { useId, useRef } from 'react'
import { cn } from '@/lib/utils'

export interface BrandPickerOption<T extends string> {
  value: T
  label: string
  /** One line under the name; omit for compact chips. */
  description?: string
  /** The service's mark, usually a decorative `BrandIcon`. */
  icon: React.ReactNode
}

/**
 * A radio group of logo tiles (trackers, channel types, SSO presets). Tab
 * reaches the selected tile; the arrow keys, Home and End move the
 * selection, as in a native radio group.
 */
export function BrandPicker<T extends string>({
  label,
  value,
  onChange,
  options,
  layout = 'grid',
  disabled,
  className,
}: {
  label: string
  value: T
  onChange: (v: T) => void
  options: BrandPickerOption<T>[]
  /** `grid`: tiles with name and description; `chips`: a wrapping row of small chips. */
  layout?: 'grid' | 'chips'
  disabled?: boolean
  className?: string
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([])
  const id = useId()
  const selected = Math.max(
    0,
    options.findIndex((o) => o.value === value),
  )
  function move(e: React.KeyboardEvent, i: number) {
    const last = options.length - 1
    const next =
      e.key === 'ArrowRight' || e.key === 'ArrowDown'
        ? i === last
          ? 0
          : i + 1
        : e.key === 'ArrowLeft' || e.key === 'ArrowUp'
          ? i === 0
            ? last
            : i - 1
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? last
              : null
    if (next === null) return
    e.preventDefault()
    onChange(options[next].value)
    refs.current[next]?.focus()
  }
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className={cn(layout === 'grid' ? 'grid grid-cols-2 gap-2 sm:grid-cols-3' : 'flex flex-wrap gap-2', className)}
      data-brand-picker
    >
      {options.map((o, i) => {
        const checked = o.value === value
        return (
          <button
            key={o.value}
            ref={(el) => {
              refs.current[i] = el
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            aria-labelledby={`${id}-${i}`}
            aria-describedby={o.description && layout === 'grid' ? `${id}-${i}-d` : undefined}
            tabIndex={i === selected ? 0 : -1}
            disabled={disabled}
            onClick={() => onChange(o.value)}
            onKeyDown={(e) => move(e, i)}
            className={cn(
              'group/tile relative flex items-center gap-2.5 rounded-lg border border-border-strong bg-card text-left outline-none',
              'transition-[transform,border-color,background-color,box-shadow] duration-150 ease-out',
              'hover:border-palm hover:bg-palm-soft/60 hover:shadow-[0_4px_14px_-6px_rgb(0_0_0/0.6)] motion-safe:hover:-translate-y-px',
              'focus-visible:border-brand focus-visible:ring-[3px] focus-visible:ring-ring/40',
              'active:translate-y-0 disabled:pointer-events-none disabled:opacity-60',
              layout === 'grid' ? 'min-h-14 px-3 py-2.5' : 'px-2.5 py-1.5',
              checked && 'border-brand bg-brand-soft hover:border-brand hover:bg-brand-soft',
            )}
          >
            {o.icon}
            <span className="min-w-0">
              <span id={`${id}-${i}`} className={cn('block text-[13px] leading-tight font-medium', checked && 'text-brand-text')}>
                {o.label}
              </span>
              {o.description && layout === 'grid' && (
                <span id={`${id}-${i}-d`} className="mt-0.5 block text-[11.5px] leading-snug text-muted-foreground">
                  {o.description}
                </span>
              )}
            </span>
          </button>
        )
      })}
    </div>
  )
}
