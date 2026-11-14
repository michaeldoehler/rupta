import { Link } from '@tanstack/react-router'
import type { Feedback } from '@/api/types'
import { UserAvatar } from '@/components/common/avatar'
import { PlatformIcon } from '@/components/common/platform-icon'
import { RelativeTime } from '@/components/common/time'

export function FeedbackItem({ f, org, showIssue = true }: { f: Feedback; org: string; showIssue?: boolean }) {
  const who = f.name ?? f.email ?? 'Anonymous'
  return (
    <li className="flex gap-3 px-4 py-3">
      <UserAvatar name={who} className="size-8" />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-x-2 text-[13px]">
          <span className="font-medium">{who}</span>
          {f.email && f.name && (
            <a href={`mailto:${f.email}`} className="text-xs text-muted-foreground hover:underline">
              {f.email}
            </a>
          )}
          <span className="ml-auto text-xs text-muted-foreground">
            <RelativeTime date={f.created_at} suffix />
          </span>
        </div>
        <p className="mt-1 text-[13.5px] leading-relaxed whitespace-pre-wrap">{f.message}</p>
        <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          <span className="inline-flex items-center gap-1">
            <PlatformIcon platform={f.project.platform} size="sm" /> {f.project.slug}
          </span>
          {showIssue && f.issue && (
            <Link to="/$org/issues/$issueId" params={{ org, issueId: f.issue.id }} className="inline-flex min-w-0 items-center gap-1 hover:text-foreground">
              <span className="font-mono">{f.issue.short_id}</span>
              <span className="max-w-72 truncate">{f.issue.title}</span>
            </Link>
          )}
          {f.event_id && f.issue && (
            <Link to="/$org/issues/$issueId" params={{ org, issueId: f.issue.id }} search={{ event: f.event_id }} className="font-mono hover:text-foreground">
              event {f.event_id.slice(0, 8)}
            </Link>
          )}
          {f.url && <span className="max-w-72 truncate font-mono">{f.url}</span>}
        </div>
      </div>
    </li>
  )
}
