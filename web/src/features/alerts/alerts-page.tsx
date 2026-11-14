import { useQueries, useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { Bell, Plus } from 'lucide-react'
import { api } from '@/api/endpoints'
import { qk, useApiMutation, useProjects, useTeams } from '@/api/queries'
import type { AlertRule } from '@/api/types'
import { PlatformIcon } from '@/components/common/platform-icon'
import { EmptyState, ErrorState, ListSkeleton } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { FilterBar } from '@/features/shell/filter-bar'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { describeAction, describeCondition, describeFilter, FREQUENCIES } from './describe'

export function AlertsPage() {
  const { org, projects: selected } = useGlobalFilters()
  const projects = useProjects(org)
  const teams = useTeams(org)
  const channels = useQuery({ queryKey: qk.channels(org), queryFn: () => api.orgs.channels(org) })
  const targets = (projects.data ?? []).filter((p) => !selected.length || selected.includes(p.id))
  const results = useQueries({ queries: targets.map((p) => ({ queryKey: qk.alertRules(org, p.slug), queryFn: () => api.projects.alertRules(org, p.slug) })) })
  const rules: AlertRule[] = results.flatMap((r) => r.data ?? [])
  const pending = projects.isPending || results.some((r) => r.isPending)
  const error = projects.error ?? results.find((r) => r.error)?.error
  const toggle = useApiMutation((r: AlertRule) => api.projects.updateAlertRule(org, r.project.slug, r.id, { enabled: !r.enabled }), {
    invalidate: targets.map((p) => qk.alertRules(org, p.slug)),
    success: (_d, r) => (r.enabled ? `“${r.name}” paused` : `“${r.name}” enabled`),
  })

  return (
    <div>
      <div className="flex flex-wrap items-center justify-between gap-2 border-b px-3 py-3 sm:px-5">
        <div className="flex items-center gap-3">
          <h1 className="text-lg font-semibold tracking-tight">Alerts</h1>
          <Button asChild size="sm" variant="brand">
            <Link to="/$org/alerts/new" params={{ org }}>
              <Plus /> New alert rule
            </Link>
          </Button>
        </div>
        <FilterBar showEnvironment={false} showPeriod={false} />
      </div>
      <div className="mx-auto max-w-5xl px-3 py-4 sm:px-5">
        {pending ? (
          <ListSkeleton rows={5} />
        ) : error ? (
          <ErrorState error={error} />
        ) : rules.length === 0 ? (
          <EmptyState
            icon={<Bell />}
            title="No alert rules"
            description="Alert rules notify you by email, Slack, Microsoft Teams or webhook when issues are created, regress or spike."
            action={
              <Button asChild size="sm" variant="outline">
                <Link to="/$org/alerts/new" params={{ org }}>
                  Create the first rule
                </Link>
              </Button>
            }
          />
        ) : (
          <ul className="divide-y rounded-lg border bg-card">
            {rules.map((r) => (
              <li key={r.id} className="flex flex-wrap items-start gap-3 px-4 py-3 sm:flex-nowrap">
                <Switch
                  checked={r.enabled}
                  onCheckedChange={() => toggle.mutate(r)}
                  aria-label={`${r.enabled ? 'Pause' : 'Enable'} ${r.name}`}
                  className="mt-0.5"
                />
                <div className="min-w-0 flex-1">
                  <Link to="/$org/alerts/$project/$ruleId" params={{ org, project: r.project.slug, ruleId: r.id }} className="font-medium hover:underline">
                    {r.name}
                  </Link>
                  <p className="mt-0.5 text-[13px] text-muted-foreground">
                    <span className="text-foreground/80">When</span>{' '}
                    {r.conditions
                      .map(describeCondition)
                      .join(r.condition_match === 'all' ? ' and ' : ' or ')
                      .toLowerCase()}
                    {r.filters.length > 0 && (
                      <>
                        , <span className="text-foreground/80">if</span> {r.filters.map(describeFilter).join(r.filter_match === 'all' ? ' and ' : ' or ')}
                      </>
                    )}
                    {r.environment && <> in {r.environment}</>}
                  </p>
                  <p className="mt-0.5 text-[13px] text-muted-foreground">
                    <span className="text-foreground/80">Then</span> {r.actions.map((a) => describeAction(a, channels.data ?? [], teams.data ?? [])).join(', ')}{' '}
                    · at most once every {FREQUENCIES.find(([m]) => m === r.frequency_minutes)?.[1] ?? `${r.frequency_minutes} minutes`}
                  </p>
                </div>
                <div className="flex shrink-0 flex-col items-end gap-1 text-xs text-muted-foreground">
                  <span className="inline-flex items-center gap-1.5">
                    <PlatformIcon platform={r.project.platform} size="sm" /> {r.project.slug}
                  </span>
                  <span>
                    {r.last_fired ? (
                      <>
                        fired <RelativeTime date={r.last_fired} suffix />
                      </>
                    ) : (
                      'never fired'
                    )}
                  </span>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  )
}
