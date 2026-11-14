import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { ChevronRight, Loader2, Plus, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { ApiError, errorMessage } from '@/api/client'
import { api } from '@/api/endpoints'
import { qk, useEnvironments, useMembers, useProjects, useTeams } from '@/api/queries'
import type { AlertAction, AlertCondition, AlertFilter, AlertRuleInput, Level } from '@/api/types'
import { Field, FormError } from '@/components/common/field'
import { PlatformIcon } from '@/components/common/platform-icon'
import { ErrorState } from '@/components/common/states'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { cn } from '@/lib/utils'
import { FREQUENCIES, INTERVALS } from './describe'

const EMPTY: AlertRuleInput = {
  name: '',
  enabled: true,
  conditions: [{ type: 'first_seen' }],
  condition_match: 'any',
  filters: [],
  filter_match: 'all',
  actions: [{ type: 'email', target: 'owners' }],
  frequency_minutes: 30,
  environment: null,
}

const LEVELS: Level[] = ['fatal', 'error', 'warning', 'info', 'debug']

function Block({
  step,
  title,
  description,
  children,
  match,
  onMatch,
}: {
  step: string
  title: string
  description: string
  children: React.ReactNode
  match?: 'any' | 'all'
  onMatch?: (m: 'any' | 'all') => void
}) {
  return (
    <section className="rounded-lg border bg-card">
      <div className="flex flex-wrap items-center gap-3 border-b px-4 py-2.5">
        <span className="rounded bg-foreground px-1.5 py-0.5 font-mono text-[11px] font-bold text-background">{step}</span>
        <div className="min-w-0 flex-1">
          <h2 className="text-[13px] font-semibold">{title}</h2>
          <p className="text-xs text-muted-foreground">{description}</p>
        </div>
        {match && onMatch && (
          <Select value={match} onValueChange={(v) => onMatch(v as 'any' | 'all')}>
            <SelectTrigger size="sm" className="w-auto" aria-label={`${title}: match`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="any">any of these</SelectItem>
              <SelectItem value="all">all of these</SelectItem>
            </SelectContent>
          </Select>
        )}
      </div>
      <div className="space-y-2 p-3">{children}</div>
    </section>
  )
}

function Row({ children, onRemove, label }: { children: React.ReactNode; onRemove: () => void; label: string }) {
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-md border bg-background px-2.5 py-2 text-[13px]">
      {children}
      <Button variant="ghost" size="icon-xs" className="ml-auto text-muted-foreground" onClick={onRemove} aria-label={`Remove ${label}`}>
        <Trash2 />
      </Button>
    </div>
  )
}

export function RuleEditor({ org, project: projectSlug, ruleId }: { org: string; project?: string; ruleId?: string }) {
  const isNew = !ruleId
  const navigate = useNavigate()
  const qc = useQueryClient()
  const projects = useProjects(org)
  const [project, setProject] = useState(projectSlug ?? '')
  const existing = useQuery({
    queryKey: qk.alertRule(org, projectSlug ?? '', ruleId ?? ''),
    queryFn: () => api.projects.alertRule(org, projectSlug!, ruleId!),
    enabled: !isNew,
  })
  const channels = useQuery({ queryKey: qk.channels(org), queryFn: () => api.orgs.channels(org) })
  const members = useMembers(org)
  const teams = useTeams(org)
  const projectId = projects.data?.find((p) => p.slug === project)?.id
  const envs = useEnvironments(org, projectId ? [projectId] : [])
  const [rule, setRule] = useState<AlertRuleInput>(EMPTY)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [formError, setFormError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)

  useEffect(() => {
    if (!project && projects.data?.length) setProject(projects.data[0].slug)
  }, [project, projects.data])
  useEffect(() => {
    if (existing.data) {
      const { id: _id, project: _p, created_by: _c, created_at: _ca, updated_at: _u, last_fired: _l, ...input } = existing.data
      setRule(input)
    }
  }, [existing.data])

  const set = <K extends keyof AlertRuleInput>(k: K, v: AlertRuleInput[K]) => setRule((r) => ({ ...r, [k]: v }))
  const setAt = <K extends 'conditions' | 'filters' | 'actions'>(k: K, i: number, v: AlertRuleInput[K][number]) =>
    setRule((r) => ({ ...r, [k]: r[k].map((x, j) => (j === i ? v : x)) }))
  const removeAt = (k: 'conditions' | 'filters' | 'actions', i: number) => setRule((r) => ({ ...r, [k]: r[k].filter((_, j) => j !== i) }))

  function validate() {
    const e: Record<string, string> = {}
    if (!rule.name.trim()) e.name = 'Give the rule a name.'
    if (!project) e.project = 'Choose a project.'
    if (!rule.conditions.length) e.conditions = 'Add at least one trigger.'
    if (!rule.actions.length) e.actions = 'Add at least one action.'
    rule.actions.forEach((a, i) => {
      if (a.type === 'channel' && !a.channel_id) e[`action-${i}`] = 'Choose a channel.'
      if (a.type === 'email' && (a.target === 'user' || a.target === 'team') && !a.target_id) e[`action-${i}`] = 'Choose who to email.'
    })
    rule.filters.forEach((f, i) => {
      if (f.type === 'tag' && !f.key.trim()) e[`filter-${i}`] = 'Enter a tag key.'
    })
    setErrors(e)
    return Object.keys(e).length === 0
  }

  async function save() {
    if (!validate()) return
    setSaving(true)
    setFormError(null)
    try {
      if (isNew) await api.projects.createAlertRule(org, project, rule)
      else await api.projects.updateAlertRule(org, projectSlug!, ruleId!, rule)
      await qc.invalidateQueries({ queryKey: ['projects', org] })
      toast.success(isNew ? 'Alert rule created' : 'Alert rule saved')
      await navigate({ to: '/$org/alerts', params: { org } })
    } catch (err) {
      if (err instanceof ApiError && Object.keys(err.fieldErrors).length) setErrors(err.fieldErrors)
      setFormError(errorMessage(err))
    } finally {
      setSaving(false)
    }
  }

  async function remove() {
    try {
      await api.projects.deleteAlertRule(org, projectSlug!, ruleId!)
      await qc.invalidateQueries({ queryKey: ['projects', org] })
      toast.success('Alert rule deleted')
      await navigate({ to: '/$org/alerts', params: { org } })
    } catch (err) {
      toast.error(errorMessage(err))
    }
  }

  if (existing.isError) return <ErrorState error={existing.error} className="min-h-[60vh]" />
  if (!isNew && existing.isPending)
    return (
      <div className="mx-auto max-w-3xl space-y-4 p-6">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-40" />
        <Skeleton className="h-40" />
      </div>
    )

  return (
    <div className="pb-24">
      <header className="border-b px-4 pt-3 pb-4 sm:px-6">
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Link to="/$org/alerts" params={{ org }} className="hover:text-foreground">
            Alerts
          </Link>
          <ChevronRight className="size-3" />
          <span>{isNew ? 'New rule' : rule.name}</span>
        </div>
        <h1 className="mt-2 text-xl font-semibold tracking-tight">{isNew ? 'New alert rule' : 'Edit alert rule'}</h1>
      </header>
      <form
        className="mx-auto max-w-3xl space-y-4 px-4 pt-5 sm:px-6"
        onSubmit={(e) => {
          e.preventDefault()
          void save()
        }}
        noValidate
      >
        <FormError message={formError} />
        <div className="grid gap-4 rounded-lg border bg-card p-4 sm:grid-cols-2">
          <Field label="Name" error={errors.name} className="sm:col-span-2">
            {(p) => <Input {...p} value={rule.name} onChange={(e) => set('name', e.target.value)} placeholder="e.g. New errors in production" />}
          </Field>
          <Field label="Project" error={errors.project}>
            {(p) => (
              <Select value={project} onValueChange={setProject} disabled={!isNew}>
                <SelectTrigger id={p.id} className="w-full" aria-invalid={p['aria-invalid']}>
                  <SelectValue placeholder="Choose a project" />
                </SelectTrigger>
                <SelectContent>
                  {projects.data?.map((pr) => (
                    <SelectItem key={pr.id} value={pr.slug}>
                      <PlatformIcon platform={pr.platform} size="sm" /> {pr.slug}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </Field>
          <Field label="Environment">
            {(p) => (
              <Select value={rule.environment ?? '__any'} onValueChange={(v) => set('environment', v === '__any' ? null : v)}>
                <SelectTrigger id={p.id} className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__any">All environments</SelectItem>
                  {envs.data?.map((e) => (
                    <SelectItem key={e} value={e}>
                      {e}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </Field>
        </div>

        <Block step="WHEN" title="Triggers" description="What starts the rule" match={rule.condition_match} onMatch={(m) => set('condition_match', m)}>
          {rule.conditions.map((c, i) => (
            <Row key={i} onRemove={() => removeAt('conditions', i)} label="trigger">
              <Select
                value={c.type}
                onValueChange={(t) =>
                  setAt(
                    'conditions',
                    i,
                    t === 'event_frequency' || t === 'user_frequency'
                      ? ({ type: t, count: 100, interval: '1h' } as AlertCondition)
                      : ({ type: t } as AlertCondition),
                  )
                }
              >
                <SelectTrigger size="sm" className="w-auto" aria-label="Trigger type">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="first_seen">A new issue is created</SelectItem>
                  <SelectItem value="regression">A resolved issue comes back</SelectItem>
                  <SelectItem value="reappeared">An ignored issue reappears</SelectItem>
                  <SelectItem value="event_frequency">An issue is seen more than…</SelectItem>
                  <SelectItem value="user_frequency">An issue affects more than…</SelectItem>
                </SelectContent>
              </Select>
              {(c.type === 'event_frequency' || c.type === 'user_frequency') && (
                <>
                  <Input
                    type="number"
                    min={1}
                    value={c.count}
                    onChange={(e) => setAt('conditions', i, { ...c, count: Math.max(1, Number(e.target.value)) })}
                    className="h-7 w-24"
                    aria-label="Threshold"
                  />
                  <span>{c.type === 'event_frequency' ? 'times in' : 'users in'}</span>
                  <Select value={c.interval} onValueChange={(v) => setAt('conditions', i, { ...c, interval: v })}>
                    <SelectTrigger size="sm" className="w-auto" aria-label="Interval">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {INTERVALS.map(([k, label]) => (
                        <SelectItem key={k} value={k}>
                          {label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </>
              )}
            </Row>
          ))}
          {errors.conditions && <p className="text-xs text-destructive">{errors.conditions}</p>}
          <Button variant="ghost" size="sm" onClick={() => set('conditions', [...rule.conditions, { type: 'regression' }])}>
            <Plus /> Add trigger
          </Button>
        </Block>

        <Block
          step="IF"
          title="Filters"
          description="Optional: only for issues that also match"
          match={rule.filter_match}
          onMatch={(m) => set('filter_match', m)}
        >
          {rule.filters.length === 0 && <p className="px-1 text-[13px] text-muted-foreground">No filters: every issue that triggers the rule counts.</p>}
          {rule.filters.map((f, i) => (
            <div key={i}>
              <Row onRemove={() => removeAt('filters', i)} label="filter">
                <Select
                  value={f.type}
                  onValueChange={(t) =>
                    setAt(
                      'filters',
                      i,
                      t === 'level'
                        ? { type: 'level', match: 'gte', level: 'error' }
                        : t === 'tag'
                          ? { type: 'tag', key: '', match: 'eq', value: '' }
                          : t === 'environment'
                            ? { type: 'environment', value: 'production' }
                            : { type: 'issue_age', older_than_minutes: 60 },
                    )
                  }
                >
                  <SelectTrigger size="sm" className="w-auto" aria-label="Filter type">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="level">Level</SelectItem>
                    <SelectItem value="tag">Tag</SelectItem>
                    <SelectItem value="environment">Environment</SelectItem>
                    <SelectItem value="issue_age">Issue age</SelectItem>
                  </SelectContent>
                </Select>
                <FilterFields f={f} onChange={(v) => setAt('filters', i, v)} />
              </Row>
              {errors[`filter-${i}`] && <p className="mt-1 text-xs text-destructive">{errors[`filter-${i}`]}</p>}
            </div>
          ))}
          <Button variant="ghost" size="sm" onClick={() => set('filters', [...rule.filters, { type: 'level', match: 'gte', level: 'error' }])}>
            <Plus /> Add filter
          </Button>
        </Block>

        <Block step="THEN" title="Actions" description="Who gets notified">
          {rule.actions.map((a, i) => (
            <div key={i}>
              <Row onRemove={() => removeAt('actions', i)} label="action">
                <Select
                  value={a.type}
                  onValueChange={(t) =>
                    setAt('actions', i, t === 'email' ? { type: 'email', target: 'owners' } : { type: 'channel', channel_id: channels.data?.[0]?.id ?? '' })
                  }
                >
                  <SelectTrigger size="sm" className="w-auto" aria-label="Action type">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="email">Send an email to</SelectItem>
                    <SelectItem value="channel">Send a notification to</SelectItem>
                  </SelectContent>
                </Select>
                {a.type === 'email' ? (
                  <>
                    <Select value={a.target} onValueChange={(t) => setAt('actions', i, { type: 'email', target: t as 'owners' })}>
                      <SelectTrigger size="sm" className="w-auto" aria-label="Recipients">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="owners">the project's team</SelectItem>
                        <SelectItem value="members">all members</SelectItem>
                        <SelectItem value="team">a team</SelectItem>
                        <SelectItem value="user">a member</SelectItem>
                      </SelectContent>
                    </Select>
                    {(a.target === 'team' || a.target === 'user') && (
                      <Select value={a.target_id ?? ''} onValueChange={(v) => setAt('actions', i, { ...a, target_id: v })}>
                        <SelectTrigger size="sm" className="w-auto min-w-40" aria-label={a.target === 'team' ? 'Team' : 'Member'}>
                          <SelectValue placeholder={a.target === 'team' ? 'Choose a team' : 'Choose a member'} />
                        </SelectTrigger>
                        <SelectContent>
                          {a.target === 'team'
                            ? teams.data?.map((t) => (
                                <SelectItem key={t.id} value={t.id}>
                                  #{t.slug}
                                </SelectItem>
                              ))
                            : members.data?.map((m) => (
                                <SelectItem key={m.user.id} value={m.user.id}>
                                  {m.user.name || m.user.email}
                                </SelectItem>
                              ))}
                        </SelectContent>
                      </Select>
                    )}
                  </>
                ) : (
                  <Select value={a.channel_id} onValueChange={(v) => setAt('actions', i, { type: 'channel', channel_id: v })}>
                    <SelectTrigger size="sm" className="w-auto min-w-48" aria-label="Channel">
                      <SelectValue placeholder="Choose a channel" />
                    </SelectTrigger>
                    <SelectContent>
                      {channels.data?.map((c) => (
                        <SelectItem key={c.id} value={c.id}>
                          {c.name} <span className="text-muted-foreground">({c.kind})</span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              </Row>
              {errors[`action-${i}`] && <p className="mt-1 text-xs text-destructive">{errors[`action-${i}`]}</p>}
            </div>
          ))}
          {errors.actions && <p className="text-xs text-destructive">{errors.actions}</p>}
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => set('actions', [...rule.actions, { type: 'channel', channel_id: channels.data?.[0]?.id ?? '' } as AlertAction])}
            >
              <Plus /> Add action
            </Button>
            <Link to="/$org/settings/channels" params={{ org }} className="text-xs text-muted-foreground hover:text-foreground hover:underline">
              Manage notification channels
            </Link>
          </div>
        </Block>

        <div className="flex flex-wrap items-center gap-4 rounded-lg border bg-card p-4">
          <div className="flex items-center gap-2 text-[13px]">
            <Label htmlFor="frequency" className="font-normal">
              Act at most once every
            </Label>
            <Select value={String(rule.frequency_minutes)} onValueChange={(v) => set('frequency_minutes', Number(v))}>
              <SelectTrigger id="frequency" size="sm" className="w-auto">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {FREQUENCIES.map(([m, label]) => (
                  <SelectItem key={m} value={String(m)}>
                    {label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <span className="text-muted-foreground">per issue</span>
          </div>
          <div className="ml-auto flex items-center gap-2">
            <Switch id="enabled" checked={rule.enabled} onCheckedChange={(v) => set('enabled', v)} />
            <Label htmlFor="enabled" className="font-normal">
              Enabled
            </Label>
          </div>
        </div>

        <div className="sticky bottom-0 -mx-4 flex items-center gap-2 border-t bg-background/95 px-4 py-3 backdrop-blur sm:-mx-6 sm:px-6">
          {!isNew && (
            <Button variant="ghost" className="text-destructive" onClick={() => setConfirmDelete(true)}>
              <Trash2 /> Delete
            </Button>
          )}
          <div className="ml-auto flex gap-2">
            <Button variant="outline" asChild>
              <Link to="/$org/alerts" params={{ org }}>
                Cancel
              </Link>
            </Button>
            <Button type="submit" variant="brand" disabled={saving}>
              {saving && <Loader2 className="animate-spin" />} {isNew ? 'Create rule' : 'Save rule'}
            </Button>
          </div>
        </div>
      </form>
      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete “{rule.name}”?</AlertDialogTitle>
            <AlertDialogDescription>No more notifications will be sent for this rule.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction destructive onClick={() => void remove()}>
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

function FilterFields({ f, onChange }: { f: AlertFilter; onChange: (f: AlertFilter) => void }) {
  if (f.type === 'level')
    return (
      <>
        <Select value={f.match} onValueChange={(m) => onChange({ ...f, match: m as 'eq' })}>
          <SelectTrigger size="sm" className="w-auto" aria-label="Comparison">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="eq">is</SelectItem>
            <SelectItem value="gte">is at least</SelectItem>
            <SelectItem value="lte">is at most</SelectItem>
          </SelectContent>
        </Select>
        <Select value={f.level} onValueChange={(l) => onChange({ ...f, level: l as Level })}>
          <SelectTrigger size="sm" className="w-auto capitalize" aria-label="Level">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {LEVELS.map((l) => (
              <SelectItem key={l} value={l} className="capitalize">
                {l}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </>
    )
  if (f.type === 'tag')
    return (
      <>
        <Input
          value={f.key}
          onChange={(e) => onChange({ ...f, key: e.target.value })}
          placeholder="tag key"
          className="h-7 w-36 font-mono text-[12.5px]"
          aria-label="Tag key"
        />
        <Select value={f.match} onValueChange={(m) => onChange({ ...f, match: m as 'eq' })}>
          <SelectTrigger size="sm" className="w-auto" aria-label="Comparison">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="eq">equals</SelectItem>
            <SelectItem value="ne">does not equal</SelectItem>
            <SelectItem value="contains">contains</SelectItem>
            <SelectItem value="starts_with">starts with</SelectItem>
            <SelectItem value="is_set">is set</SelectItem>
          </SelectContent>
        </Select>
        {f.match !== 'is_set' && (
          <Input
            value={f.value ?? ''}
            onChange={(e) => onChange({ ...f, value: e.target.value })}
            placeholder="value"
            className={cn('h-7 w-40 font-mono text-[12.5px]')}
            aria-label="Tag value"
          />
        )}
      </>
    )
  if (f.type === 'environment')
    return (
      <>
        <span>is</span>
        <Input value={f.value} onChange={(e) => onChange({ ...f, value: e.target.value })} className="h-7 w-40" aria-label="Environment" />
      </>
    )
  return (
    <>
      <span>older than</span>
      <Input
        type="number"
        min={1}
        value={f.older_than_minutes}
        onChange={(e) => onChange({ ...f, older_than_minutes: Math.max(1, Number(e.target.value)) })}
        className="h-7 w-24"
        aria-label="Minutes"
      />
      <span>minutes</span>
    </>
  )
}
