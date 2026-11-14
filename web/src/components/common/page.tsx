import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'

export function PageHeader({
  title,
  description,
  actions,
  className,
  children,
}: {
  title: ReactNode
  description?: ReactNode
  actions?: ReactNode
  className?: string
  children?: ReactNode
}) {
  return (
    <div className={cn('flex flex-col gap-3 border-b px-4 pt-4 pb-3 sm:px-6', className)}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h1 className="text-lg font-semibold tracking-tight">{title}</h1>
          {description && <p className="mt-0.5 text-[13px] text-muted-foreground">{description}</p>}
        </div>
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {children}
    </div>
  )
}

export function Section({
  title,
  description,
  actions,
  children,
  className,
  id,
}: {
  title: ReactNode
  description?: ReactNode
  actions?: ReactNode
  children: ReactNode
  className?: string
  id?: string
}) {
  return (
    <section id={id} className={cn('scroll-mt-16', className)}>
      <div className="mb-2 flex items-end justify-between gap-2">
        <div>
          <h2 className="text-[13px] font-semibold">{title}</h2>
          {description && <p className="text-xs text-muted-foreground">{description}</p>}
        </div>
        {actions}
      </div>
      {children}
    </section>
  )
}

export function KeyValueTable({ rows, className, mono = true }: { rows: [string, ReactNode][]; className?: string; mono?: boolean }) {
  return (
    <div className={cn('overflow-hidden rounded-lg border bg-card', className)}>
      <table className="w-full table-fixed text-[13px]">
        <tbody className="divide-y">
          {rows.map(([k, v], i) => (
            <tr key={`${k}-${i}`} className="align-top">
              <th scope="row" className="w-[34%] max-w-56 truncate bg-muted/40 px-3 py-1.5 text-left font-normal text-muted-foreground" title={k}>
                {k}
              </th>
              <td className={cn('px-3 py-1.5 break-words whitespace-pre-wrap', mono && 'font-mono text-[12.5px]')}>{v}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
