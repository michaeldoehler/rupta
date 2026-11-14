import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'
import { EnvironmentSelector, PeriodSelector, ProjectSelector } from './selectors'

/** Top bar of list pages: global selectors on the left, page actions on the right. */
export function FilterBar({
  children,
  className,
  showEnvironment = true,
  showPeriod = true,
}: {
  children?: ReactNode
  className?: string
  showEnvironment?: boolean
  showPeriod?: boolean
}) {
  return (
    <div className={cn('flex flex-wrap items-center gap-2', className)}>
      <ProjectSelector />
      {showEnvironment && <EnvironmentSelector />}
      {showPeriod && <PeriodSelector />}
      {children}
    </div>
  )
}
