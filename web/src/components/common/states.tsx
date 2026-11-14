import { AlertTriangle, RefreshCw, SearchX } from 'lucide-react'
import type { ReactNode } from 'react'
import { ApiError, errorMessage } from '@/api/client'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
  compact = false,
}: {
  icon?: ReactNode
  title: string
  description?: ReactNode
  action?: ReactNode
  className?: string
  compact?: boolean
}) {
  return (
    <div className={cn('flex flex-col items-center justify-center text-center', compact ? 'gap-2 px-4 py-8' : 'gap-3 px-6 py-16', className)}>
      {icon && <div className="flex size-11 items-center justify-center rounded-xl border bg-card text-muted-foreground shadow-xs [&_svg]:size-5">{icon}</div>}
      <div className="space-y-1">
        <p className="text-sm font-semibold">{title}</p>
        {description && <p className="mx-auto max-w-md text-[13px] text-muted-foreground">{description}</p>}
      </div>
      {action}
    </div>
  )
}

export function ErrorState({ error, retry, className, title }: { error: unknown; retry?: () => void; className?: string; title?: string }) {
  const notFound = error instanceof ApiError && error.isNotFound
  const forbidden = error instanceof ApiError && error.isForbidden
  return (
    <EmptyState
      className={className}
      icon={notFound ? <SearchX /> : <AlertTriangle className="text-destructive" />}
      title={title ?? (notFound ? 'Not found' : forbidden ? 'No access' : 'Could not load this')}
      description={errorMessage(error)}
      action={
        retry && !notFound && !forbidden ? (
          <Button variant="outline" size="sm" onClick={retry}>
            <RefreshCw /> Try again
          </Button>
        ) : undefined
      }
    />
  )
}

export function ListSkeleton({ rows = 8, className }: { rows?: number; className?: string }) {
  return (
    <div className={cn('divide-y', className)} aria-busy="true" aria-label="Loading">
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="flex items-center gap-3 px-4 py-3">
          <Skeleton className="h-4 w-4" />
          <div className="flex-1 space-y-1.5">
            <Skeleton className="h-3.5" style={{ width: `${40 + ((i * 37) % 45)}%` }} />
            <Skeleton className="h-3 w-1/3" />
          </div>
          <Skeleton className="h-5 w-20" />
        </div>
      ))}
    </div>
  )
}

export function CardsSkeleton({ count = 3, className }: { count?: number; className?: string }) {
  return (
    <div className={cn('grid gap-3', className)} aria-busy="true" aria-label="Loading">
      {Array.from({ length: count }, (_, i) => (
        <Skeleton key={i} className="h-24 w-full rounded-lg" />
      ))}
    </div>
  )
}
