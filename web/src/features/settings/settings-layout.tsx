import { Link } from '@tanstack/react-router'
import type { ReactNode } from 'react'
import { useProjects } from '@/api/queries'
import { PlatformIcon } from '@/components/common/platform-icon'
import { cn } from '@/lib/utils'

const SECTIONS = [
  {
    title: 'Account',
    items: [
      { to: '/$org/settings/account', label: 'Profile & preferences', exact: true },
      { to: '/$org/settings/account/security', label: 'Password & tokens' },
      { to: '/$org/settings/account/notifications', label: 'Notifications' },
    ],
  },
  {
    title: 'Organization',
    items: [
      { to: '/$org/settings', label: 'General', exact: true },
      { to: '/$org/settings/members', label: 'Members' },
      { to: '/$org/settings/teams', label: 'Teams' },
      { to: '/$org/settings/sso', label: 'Single sign-on' },
      { to: '/$org/settings/tokens', label: 'API tokens' },
      { to: '/$org/settings/integrations', label: 'Integrations' },
      { to: '/$org/settings/channels', label: 'Notification channels' },
      { to: '/$org/settings/audit-log', label: 'Audit log' },
    ],
  },
] as const

export function SettingsLayout({ org, children }: { org: string; children: ReactNode }) {
  const projects = useProjects(org)
  const linkCls =
    'block shrink-0 rounded-md px-2 py-1.5 text-[13px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none'
  return (
    <div className="flex flex-col md:flex-row">
      <aside
        className="shrink-0 border-b md:sticky md:top-0 md:h-[calc(100svh-3rem)] md:w-56 md:overflow-y-auto md:border-r md:border-b-0"
        aria-label="Settings navigation"
      >
        <nav className="flex gap-1 overflow-x-auto p-2 md:flex-col md:gap-4 md:p-3">
          {SECTIONS.map((s) => (
            <div key={s.title} className="flex gap-1 md:block md:space-y-0.5">
              <p className="hidden px-2 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase md:block">{s.title}</p>
              {s.items.map((i) => (
                <Link
                  key={i.to}
                  to={i.to}
                  params={{ org }}
                  activeOptions={{ exact: 'exact' in i && i.exact, includeSearch: false }}
                  className={linkCls}
                  activeProps={{ className: 'bg-accent text-foreground font-medium', 'aria-current': 'page' }}
                >
                  {i.label}
                </Link>
              ))}
            </div>
          ))}
          <div className="flex gap-1 md:block md:space-y-0.5">
            <p className="hidden px-2 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase md:block">Projects</p>
            <Link
              to="/$org/settings/projects"
              params={{ org }}
              activeOptions={{ exact: true, includeSearch: false }}
              className={linkCls}
              activeProps={{ className: 'bg-accent text-foreground font-medium', 'aria-current': 'page' }}
            >
              All projects
            </Link>
            {projects.data?.map((p) => (
              <Link
                key={p.id}
                to="/$org/settings/projects/$project"
                params={{ org, project: p.slug }}
                activeOptions={{ includeSearch: false }}
                className={cn(linkCls, 'flex items-center gap-2')}
                activeProps={{ className: 'bg-accent text-foreground font-medium', 'aria-current': 'page' }}
              >
                <PlatformIcon platform={p.platform} size="sm" /> <span className="truncate">{p.slug}</span>
              </Link>
            ))}
          </div>
        </nav>
      </aside>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  )
}

export function SettingsPage({ title, description, actions, children }: { title: string; description?: ReactNode; actions?: ReactNode; children: ReactNode }) {
  return (
    <div className="mx-auto max-w-4xl px-4 py-6 sm:px-8">
      <div className="mb-6 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-lg font-semibold tracking-tight">{title}</h1>
          {description && <p className="mt-1 max-w-2xl text-[13px] text-muted-foreground">{description}</p>}
        </div>
        {actions}
      </div>
      <div className="space-y-6">{children}</div>
    </div>
  )
}

export function SettingsCard({
  title,
  description,
  children,
  footer,
  danger,
}: {
  title?: ReactNode
  description?: ReactNode
  children: ReactNode
  footer?: ReactNode
  danger?: boolean
}) {
  return (
    <section className={cn('rounded-lg border bg-card', danger && 'border-destructive/40')}>
      {(title || description) && (
        <div className="border-b px-4 py-3">
          {title && <h2 className={cn('text-sm font-semibold', danger && 'text-destructive')}>{title}</h2>}
          {description && <p className="mt-0.5 text-[13px] text-muted-foreground">{description}</p>}
        </div>
      )}
      <div className="p-4">{children}</div>
      {footer && <div className="flex items-center justify-end gap-2 border-t bg-muted/30 px-4 py-2.5">{footer}</div>}
    </section>
  )
}

/** A labelled switch row for boolean settings. */
export function ToggleRow({ title, description, children }: { title: string; description?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-4 py-3 first:pt-0 last:pb-0">
      <div className="min-w-0">
        <div className="text-[13px] font-medium">{title}</div>
        {description && <div className="mt-0.5 text-xs text-muted-foreground">{description}</div>}
      </div>
      {children}
    </div>
  )
}
