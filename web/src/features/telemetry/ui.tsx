// Page building blocks shared by the explore pages: header with the global
// selectors, section tabs, stat tiles, panels and an infinite virtual list.

import { Link, type LinkProps } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import { ArrowDown, ArrowUp } from 'lucide-react'
import { useEffect, useRef, type ReactNode } from 'react'
import { Skeleton } from '@/components/ui/skeleton'
import { PERIODS } from '@/lib/search-params'
import { cn } from '@/lib/utils'
import { FilterBar } from '@/features/shell/filter-bar'

export function ExploreHeader({
  title,
  description,
  actions,
  tabs,
  children,
  showEnvironment = true,
  className,
}: {
  title: ReactNode
  description?: ReactNode
  actions?: ReactNode
  tabs?: ReactNode
  children?: ReactNode
  showEnvironment?: boolean
  className?: string
}) {
  return (
    <div className={cn('space-y-3 border-b px-3 pt-3 sm:px-5', tabs ? 'pb-0' : 'pb-3', className)}>
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <h1 className="text-lg font-semibold tracking-tight">{title}</h1>
          {description && <p className="text-[13px] text-muted-foreground">{description}</p>}
        </div>
        <FilterBar periods={PERIODS} showEnvironment={showEnvironment}>
          {actions}
        </FilterBar>
      </div>
      {children}
      {tabs}
    </div>
  )
}

/** Underlined section tabs, like the issue sections. */
export function SectionTabs({
  label,
  items,
}: {
  label: string
  items: { to: LinkProps['to']; params?: Record<string, string>; label: string; exact?: boolean; search?: Record<string, unknown> }[]
}) {
  return (
    <nav aria-label={label} className="-mx-1 flex gap-1 overflow-x-auto">
      {items.map((it) => (
        <Link
          key={String(it.to)}
          to={it.to}
          params={it.params as never}
          search={it.search as never}
          activeOptions={{ exact: it.exact ?? false, includeSearch: false }}
          className="flex shrink-0 items-center gap-1.5 border-b-2 border-transparent px-2.5 py-2 text-[13px] font-medium text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          activeProps={{ className: 'border-brand! text-foreground', 'aria-current': 'page' }}
        >
          {it.label}
        </Link>
      ))}
    </nav>
  )
}

export function StatTile({
  label,
  value,
  hint,
  className,
  valueClassName,
}: {
  label: string
  value: ReactNode
  hint?: ReactNode
  className?: string
  valueClassName?: string
}) {
  return (
    <div className={cn('min-w-0 rounded-lg border bg-card px-3 py-2.5', className)}>
      <div className="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{label}</div>
      <div className={cn('mt-0.5 truncate text-lg font-semibold tabular', valueClassName)}>{value}</div>
      {hint && <div className="truncate text-xs text-muted-foreground">{hint}</div>}
    </div>
  )
}

export function StatTilesSkeleton({ count = 5 }: { count?: number }) {
  return (
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-6" aria-busy="true" aria-label="Loading">
      {Array.from({ length: count }, (_, i) => (
        <Skeleton key={i} className="h-[66px] rounded-lg" />
      ))}
    </div>
  )
}

export function Panel({
  title,
  description,
  actions,
  children,
  className,
  bodyClassName,
}: {
  title: ReactNode
  description?: ReactNode
  actions?: ReactNode
  children: ReactNode
  className?: string
  bodyClassName?: string
}) {
  return (
    <section className={cn('min-w-0 rounded-lg border bg-card', className)}>
      <div className="flex flex-wrap items-center justify-between gap-2 border-b px-3 py-2">
        <div className="min-w-0">
          <h2 className="text-[13px] font-semibold">{title}</h2>
          {description && <p className="text-xs text-muted-foreground">{description}</p>}
        </div>
        {actions}
      </div>
      <div className={cn('p-3', bodyClassName)}>{children}</div>
    </section>
  )
}

/** A column header that sorts (server or client side). */
export function SortButton({
  label,
  active,
  desc = true,
  onClick,
  className,
}: {
  label: string
  active: boolean
  desc?: boolean
  onClick: () => void
  className?: string
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'inline-flex items-center gap-1 rounded-sm uppercase hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none',
        active && 'text-foreground',
        className,
      )}
      aria-label={`Sort by ${label}`}
      aria-pressed={active}
    >
      {label}
      {active ? desc ? <ArrowDown className="size-3" /> : <ArrowUp className="size-3" /> : null}
    </button>
  )
}

/** A thin horizontal share bar (0..1). */
export function ShareBar({ value, color = 'var(--series-2)', className }: { value: number; color?: string; className?: string }) {
  return (
    <span className={cn('block h-1.5 w-full overflow-hidden rounded-full bg-muted', className)} aria-hidden>
      <span className="block h-full rounded-full" style={{ width: `${Math.max(0, Math.min(1, value)) * 100}%`, background: color }} />
    </span>
  )
}

/**
 * A virtualised list that loads more pages as it scrolls. Rows are divs in
 * a `grid`, so the list stays fast with thousands of entries.
 */
export function InfiniteVirtualList<T>({
  rows,
  rowHeight,
  render,
  getKey,
  hasNextPage,
  fetchNextPage,
  isFetchingNextPage,
  label,
  header,
  className,
  focusedIndex,
}: {
  rows: T[]
  rowHeight: number
  render: (row: T, index: number) => ReactNode
  getKey: (row: T) => string
  hasNextPage?: boolean
  fetchNextPage?: () => void
  isFetchingNextPage?: boolean
  label: string
  header?: ReactNode
  className?: string
  focusedIndex?: number
}) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const count = rows.length + (hasNextPage ? 1 : 0)
  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    initialRect: { width: 1200, height: 800 },
    overscan: 10,
    getItemKey: (i) => (rows[i] ? getKey(rows[i]) : `loader-${i}`),
  })
  const items = virtualizer.getVirtualItems()
  const last = items[items.length - 1]
  useEffect(() => {
    if (last && last.index >= rows.length - 10 && hasNextPage && !isFetchingNextPage) fetchNextPage?.()
  }, [last, rows.length, hasNextPage, isFetchingNextPage, fetchNextPage])
  useEffect(() => {
    if (focusedIndex !== undefined && focusedIndex >= 0) virtualizer.scrollToIndex(focusedIndex, { align: 'auto' })
  }, [focusedIndex, virtualizer])
  return (
    <div ref={scrollRef} className={cn('min-h-0 flex-1 overflow-auto', className)}>
      {header}
      <div role="grid" aria-label={label} aria-rowcount={rows.length} className="relative" style={{ height: virtualizer.getTotalSize() }}>
        {items.map((v) => {
          const row = rows[v.index]
          return (
            <div
              key={v.key}
              role="row"
              aria-rowindex={v.index + 1}
              className="absolute inset-x-0"
              style={{ height: v.size, transform: `translateY(${v.start}px)` }}
            >
              {row ? (
                render(row, v.index)
              ) : (
                <div className="flex h-full items-center px-4" aria-label="Loading more">
                  <Skeleton className="h-3.5 w-1/3" />
                </div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}
