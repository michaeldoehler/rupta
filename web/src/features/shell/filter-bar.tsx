import type { ReactNode } from 'react'
import type { Period } from '@/lib/search-params'
import { cn } from '@/lib/utils'
import { EnvironmentSelector, PeriodSelector, ProjectSelector } from './selectors'

/** Top bar of list pages: global selectors on the left, page actions on the right. */
export function FilterBar({
  children,
  className,
  showEnvironment = true,
  showPeriod = true,
  periods,
}: {
  children?: ReactNode
  className?: string
  showEnvironment?: boolean
  showPeriod?: boolean
  /** Period choices; the issue pages' set by default. */
  periods?: readonly Period[]
}) {
  return (
    <div className={cn('flex flex-wrap items-center gap-2', className)}>
      <ProjectSelector />
      {showEnvironment && <EnvironmentSelector />}
      {showPeriod && <PeriodSelector options={periods} />}
      {children}
    </div>
  )
}
