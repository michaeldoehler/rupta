import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { Plug, RefreshCw, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { ik, integrationsApi, type Integration, type ProjectIntegration, type Target } from '@/api/integrations'
import { Field } from '@/components/common/field'
import { CardsSkeleton, EmptyState, ErrorState } from '@/components/common/states'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { useRole } from '@/features/settings/org-pages'
import { SettingsCard, SettingsPage, ToggleRow } from '@/features/settings/settings-layout'
import { kindMeta, TrackerIcon } from './kinds'

const NONE = '__none'

/** Targets of an integration, read live from the tracker. */
export function useTargets(org: string, integrationId: string | null) {
  return useQuery({
    queryKey: ik.targets(org, integrationId ?? ''),
    queryFn: () => integrationsApi.targets(org, integrationId!),
    enabled: Boolean(integrationId),
    staleTime: 5 * 60_000,
    retry: false,
  })
}

function DefaultsCard({
  org,
  project,
  integration,
  current,
  can,
}: {
  org: string
  project: string
  integration: Integration
  current: ProjectIntegration | undefined
  can: boolean
}) {
  const qc = useQueryClient()
  const m = kindMeta(integration.kind)
  const targets = useTargets(org, integration.enabled ? integration.id : null)
  const [target, setTarget] = useState(current?.target ?? '')
  const [issueType, setIssueType] = useState(current?.issue_type ?? '')
  const [linearProject, setLinearProject] = useState(current?.project ?? '')
  const [labels, setLabels] = useState((current?.labels ?? []).join(', '))
  const [incoming, setIncoming] = useState(current?.incoming_sync ?? true)
  const [transition, setTransition] = useState(current?.outgoing_transition ?? true)
  const [comment, setComment] = useState(current?.outgoing_comment ?? true)
  useEffect(() => {
    setTarget(current?.target ?? '')
    setIssueType(current?.issue_type ?? '')
    setLinearProject(current?.project ?? '')
    setLabels((current?.labels ?? []).join(', '))
    setIncoming(current?.incoming_sync ?? true)
    setTransition(current?.outgoing_transition ?? true)
    setComment(current?.outgoing_comment ?? true)
  }, [current])
  const t: Target | undefined = targets.data?.find((x) => x.id === target)
  const invalidate = () => qc.invalidateQueries({ queryKey: ik.projectDefaults(org, project) })
  const save = useMutation({
    mutationFn: () =>
      integrationsApi.saveProjectDefaults(org, project, integration.id, {
        target,
        target_name: t?.name ?? current?.target_name ?? null,
        issue_type: issueType || null,
        project: linearProject || null,
        labels: labels
          .split(',')
          .map((l) => l.trim())
          .filter(Boolean),
        incoming_sync: incoming,
        outgoing_transition: transition,
        outgoing_comment: comment,
      }),
    onSuccess: () => toast.success(`Defaults for ${integration.name} saved`),
    onError: (e) => toast.error('Could not save', { description: errorMessage(e) }),
    onSettled: invalidate,
  })
  const remove = useMutation({
    mutationFn: () => integrationsApi.removeProjectDefaults(org, project, integration.id),
    onSuccess: () => toast.success('Defaults removed'),
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: invalidate,
  })
  const isJira = integration.kind === 'jira_cloud' || integration.kind === 'jira_server'
  const isLinear = integration.kind === 'linear'
  return (
    <SettingsCard
      title={
        <span className="flex items-center gap-2">
          <TrackerIcon kind={integration.kind} /> {integration.name}
          {!integration.enabled && <span className="text-xs font-normal text-muted-foreground">(disabled)</span>}
        </span>
      }
      description={
        current ? `New tickets go to ${current.target_name ?? current.target}.` : `Not set up for this project. Tickets need a ${m.targetLabel.toLowerCase()}.`
      }
      footer={
        can && (
          <>
            {current && (
              <Button variant="ghost" size="sm" onClick={() => remove.mutate()} disabled={remove.isPending}>
                <Trash2 /> Remove
              </Button>
            )}
            <Button size="sm" onClick={() => save.mutate()} disabled={!target || save.isPending}>
              Save defaults
            </Button>
          </>
        )
      }
    >
      <div className="grid gap-4 sm:grid-cols-2">
        <Field
          label={m.targetLabel}
          className="sm:col-span-2"
          hint={
            targets.isError
              ? `Could not read the list from ${m.label}: ${errorMessage(targets.error)}. Enter the ${isJira ? 'project key' : isLinear ? 'team id' : 'path'} instead.`
              : undefined
          }
        >
          {(p) =>
            targets.data && targets.data.length > 0 ? (
              <div className="flex gap-2">
                <Select value={target || NONE} onValueChange={(v) => setTarget(v === NONE ? '' : v)} disabled={!can}>
                  <SelectTrigger id={p.id} className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={NONE}>Choose…</SelectItem>
                    {targets.data.map((x) => (
                      <SelectItem key={x.id} value={x.id}>
                        {x.name}
                        {x.key && x.key !== x.name && <span className="ml-1 font-mono text-xs text-muted-foreground">{x.key}</span>}
                      </SelectItem>
                    ))}
                    {target && !targets.data.some((x) => x.id === target) && <SelectItem value={target}>{current?.target_name ?? target}</SelectItem>}
                  </SelectContent>
                </Select>
                <Button variant="outline" size="icon" onClick={() => void targets.refetch()} aria-label="Reload the list" disabled={targets.isFetching}>
                  <RefreshCw className={targets.isFetching ? 'animate-spin' : undefined} />
                </Button>
              </div>
            ) : (
              <Input
                {...p}
                value={target}
                onChange={(e) => setTarget(e.target.value)}
                disabled={!can}
                placeholder={targets.isPending && integration.enabled ? 'Loading…' : isJira ? 'OPS' : isLinear ? 'team id' : 'owner/repo'}
                className="font-mono text-[13px]"
              />
            )
          }
        </Field>
        {isJira && (
          <Field label="Issue type" optional hint="Default: Bug, or the first type that is not a sub-task.">
            {(p) =>
              t && t.issue_types.length ? (
                <Select value={issueType || NONE} onValueChange={(v) => setIssueType(v === NONE ? '' : v)} disabled={!can}>
                  <SelectTrigger id={p.id} className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={NONE}>Default</SelectItem>
                    {t.issue_types.map((x) => (
                      <SelectItem key={x.id} value={x.name}>
                        {x.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : (
                <Input {...p} value={issueType} onChange={(e) => setIssueType(e.target.value)} disabled={!can} placeholder="Bug" />
              )
            }
          </Field>
        )}
        {isLinear && (
          <Field label="Linear project" optional>
            {(p) => (
              <Select value={linearProject || NONE} onValueChange={(v) => setLinearProject(v === NONE ? '' : v)} disabled={!can || !t}>
                <SelectTrigger id={p.id} className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NONE}>None</SelectItem>
                  {t?.projects.map((x) => (
                    <SelectItem key={x.id} value={x.id}>
                      {x.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </Field>
        )}
        <Field
          label="Labels"
          optional
          hint={
            isLinear && t?.labels.length
              ? `Known: ${t.labels
                  .slice(0, 6)
                  .map((l) => l.name)
                  .join(', ')}`
              : 'Separate with commas.'
          }
          className={isJira || isLinear ? undefined : 'sm:col-span-2'}
        >
          {(p) => <Input {...p} value={labels} onChange={(e) => setLabels(e.target.value)} disabled={!can} placeholder="rupta, bug" />}
        </Field>
      </div>
      <div className="mt-4 divide-y border-t pt-3">
        <ToggleRow
          title={`Resolve and reopen issues from ${m.label}`}
          description={`When the ticket is done in ${m.label}, the rupta issue is resolved; reopened, it is unresolved. Needs the webhook.`}
        >
          <Switch checked={incoming} onCheckedChange={setIncoming} disabled={!can} aria-label="Incoming status sync" />
        </ToggleRow>
        <ToggleRow title={`Move tickets in ${m.label}`} description="Resolving the issue moves the ticket to done; unresolving it or a regression reopens it.">
          <Switch checked={transition} onCheckedChange={setTransition} disabled={!can} aria-label="Outgoing transitions" />
        </ToggleRow>
        <ToggleRow title="Comment on tickets" description="The same events add a comment with a link to the issue.">
          <Switch checked={comment} onCheckedChange={setComment} disabled={!can} aria-label="Outgoing comments" />
        </ToggleRow>
      </div>
    </SettingsCard>
  )
}

export function ProjectIntegrationsPage({ org, project }: { org: string; project: string }) {
  const integrations = useQuery({ queryKey: ik.list(org), queryFn: () => integrationsApi.list(org) })
  const defaults = useQuery({ queryKey: ik.projectDefaults(org, project), queryFn: () => integrationsApi.projectDefaults(org, project) })
  const { can } = useRole('admin')
  return (
    <SettingsPage title="Integrations" description="Where tickets created from this project's issues go, and how their status syncs.">
      {integrations.isPending || defaults.isPending ? (
        <CardsSkeleton count={2} />
      ) : integrations.isError ? (
        <ErrorState error={integrations.error} retry={() => integrations.refetch()} />
      ) : integrations.data.length === 0 ? (
        <SettingsCard>
          <EmptyState
            compact
            icon={<Plug />}
            title="No issue trackers connected"
            description="An admin connects trackers for the whole organization; then you choose the defaults for this project here."
            action={
              <Button asChild size="sm" variant="outline">
                <Link to="/$org/settings/integrations" params={{ org }}>
                  Open integrations
                </Link>
              </Button>
            }
          />
        </SettingsCard>
      ) : (
        integrations.data.map((i) => (
          <DefaultsCard key={i.id} org={org} project={project} integration={i} current={defaults.data?.find((d) => d.integration.id === i.id)} can={can} />
        ))
      )}
    </SettingsPage>
  )
}
