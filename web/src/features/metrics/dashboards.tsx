import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { ArrowLeft, ArrowRight, ChevronRight, Copy, Ellipsis, LayoutDashboard, Pencil, Plus, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { telemetryApi, tk, useMetricNames } from '@/api/telemetry'
import type { ChartType, Dashboard, MetricOp, Widget, WidgetKind } from '@/api/telemetry-types'
import { Field } from '@/components/common/field'
import { CardsSkeleton, EmptyState, ErrorState } from '@/components/common/states'
import { RelativeTime } from '@/components/common/time'
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
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { METRICS_TABS } from './metrics-page'
import { ExploreHeader, SectionTabs } from '@/features/telemetry/ui'
import { OP_LABEL, OPS_BY_TYPE, WidgetView } from './widget-view'

const KIND_LABEL: Record<WidgetKind, string> = { metric: 'Metric', transactions: 'Transaction', logs: 'Log volume', issues: 'Issues' }
const SPAN: Record<number, string> = { 1: 'md:col-span-1', 2: 'md:col-span-2', 3: 'md:col-span-3', 4: 'md:col-span-4' }

function newId() {
  return `w${Math.random().toString(36).slice(2, 9)}`
}

export function DashboardsPage() {
  const { org } = useGlobalFilters()
  const navigate = useNavigate()
  const qc = useQueryClient()
  const q = useQuery({ queryKey: tk.dashboards(org), queryFn: () => telemetryApi.dashboards.list(org) })
  const [creating, setCreating] = useState(false)
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const create = useMutation({
    mutationFn: () => telemetryApi.dashboards.create(org, { title: title.trim(), description: description.trim() || null, widgets: [] }),
    onSuccess: (d) => {
      void qc.invalidateQueries({ queryKey: tk.dashboards(org) })
      setCreating(false)
      void navigate({ to: '/$org/dashboards/$dashboardId', params: { org, dashboardId: d.id } })
    },
    onError: (e) => toast.error('Could not create the dashboard', { description: errorMessage(e) }),
  })
  return (
    <div>
      <ExploreHeader
        title="Dashboards"
        tabs={<SectionTabs label="Metrics sections" items={METRICS_TABS(org)} />}
        actions={
          <Button size="sm" onClick={() => setCreating(true)}>
            <Plus /> New dashboard
          </Button>
        }
      />
      <div className="p-3 sm:p-5">
        {q.isPending ? (
          <CardsSkeleton count={3} className="sm:grid-cols-3" />
        ) : q.isError ? (
          <ErrorState error={q.error} retry={() => q.refetch()} />
        ) : q.data.length === 0 ? (
          <EmptyState icon={<LayoutDashboard />} title="No dashboards yet" description="Put metrics, transaction latency, log volume and issues side by side, with one time range for all." action={<Button size="sm" onClick={() => setCreating(true)}><Plus /> New dashboard</Button>} />
        ) : (
          <ul className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
            {q.data.map((d) => (
              <li key={d.id}>
                <Link to="/$org/dashboards/$dashboardId" params={{ org, dashboardId: d.id }} className="group block h-full rounded-lg border bg-card p-4 transition-colors hover:border-foreground/20 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none">
                  <div className="flex items-start justify-between gap-2">
                    <h2 className="font-semibold group-hover:text-brand-text">{d.title}</h2>
                    <ChevronRight className="size-4 text-muted-foreground" />
                  </div>
                  {d.description && <p className="mt-1 line-clamp-2 text-[13px] text-muted-foreground">{d.description}</p>}
                  <div className="mt-3 grid grid-cols-4 gap-1" aria-hidden>
                    {d.widgets.slice(0, 8).map((w) => (
                      <span key={w.id} className={cn('h-5 rounded-sm bg-muted', w.width >= 2 && 'col-span-2', w.width >= 3 && 'col-span-3', w.width >= 4 && 'col-span-4')} />
                    ))}
                  </div>
                  <p className="mt-3 text-xs text-muted-foreground">
                    {d.widgets.length} {d.widgets.length === 1 ? 'widget' : 'widgets'} · updated <RelativeTime date={d.updated_at} suffix plain />
                  </p>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </div>
      <Dialog open={creating} onOpenChange={setCreating}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>New dashboard</DialogTitle>
            <DialogDescription>Dashboards are shared with everyone in the organization.</DialogDescription>
          </DialogHeader>
          <form
            className="space-y-3"
            onSubmit={(e) => {
              e.preventDefault()
              if (title.trim()) create.mutate()
            }}
          >
            <Field label="Title">{(p) => <Input {...p} autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="e.g. Checkout health" />}</Field>
            <Field label="Description" optional>
              {(p) => <Textarea {...p} value={description} onChange={(e) => setDescription(e.target.value)} rows={2} />}
            </Field>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setCreating(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={!title.trim() || create.isPending}>
                Create
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  )
}

export function WidgetEditor({ open, onOpenChange, initial, onSave }: { open: boolean; onOpenChange: (o: boolean) => void; initial: Widget | null; onSave: (w: Widget) => void }) {
  const { org, projects, environments, period } = useGlobalFilters()
  const names = useMetricNames(org, { project: projects, period })
  const [w, setW] = useState<Widget>(initial ?? { id: newId(), title: '', kind: 'metric', chart: 'line', query: {}, width: 2 })
  useEffect(() => {
    if (open) setW(initial ?? { id: newId(), title: '', kind: 'metric', chart: 'line', query: {}, width: 2 })
  }, [open, initial])
  const meta = names.data?.find((n) => n.name === w.query.name)
  const ops = meta ? OPS_BY_TYPE[meta.type] : (['sum'] as MetricOp[])
  const set = (patch: Partial<Widget>) => setW((x) => ({ ...x, ...patch }))
  const setQ = (patch: Partial<Widget['query']>) => setW((x) => ({ ...x, query: { ...x.query, ...patch } }))
  const valid = w.kind === 'metric' ? Boolean(w.query.name) : w.kind === 'transactions' ? Boolean(w.query.transaction) : true
  const autoTitle =
    w.kind === 'metric' ? `${OP_LABEL[w.query.op ?? ops[0]]} of ${w.query.name ?? '…'}${w.query.group_by ? ` by ${w.query.group_by}` : ''}` : w.kind === 'transactions' ? `${w.query.field ?? 'p95'} of ${w.query.transaction ?? '…'}` : w.kind === 'logs' ? `Logs ${w.query.query ?? ''}`.trim() : 'Issues'
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>{initial ? 'Edit widget' : 'Add widget'}</DialogTitle>
          <DialogDescription>The dashboard's period and project selection apply to every widget.</DialogDescription>
        </DialogHeader>
        <div className="grid gap-5 md:grid-cols-[minmax(0,320px)_minmax(0,1fr)]">
          <div className="space-y-3">
            <Field label="Data">
              {(p) => (
                <Select value={w.kind} onValueChange={(v) => set({ kind: v as WidgetKind, query: {} })}>
                  <SelectTrigger id={p.id} className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {(Object.keys(KIND_LABEL) as WidgetKind[]).map((k) => (
                      <SelectItem key={k} value={k}>
                        {KIND_LABEL[k]}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            </Field>
            {w.kind === 'metric' && (
              <>
                <Field label="Metric">
                  {(p) => (
                    <Select value={w.query.name ?? ''} onValueChange={(v) => setQ({ name: v, op: undefined, group_by: undefined })}>
                      <SelectTrigger id={p.id} className="w-full font-mono">
                        <SelectValue placeholder="Choose a metric" />
                      </SelectTrigger>
                      <SelectContent>
                        {names.data?.map((n) => (
                          <SelectItem key={n.name} value={n.name} className="font-mono">
                            {n.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  )}
                </Field>
                <div className="grid grid-cols-2 gap-2">
                  <Field label="Aggregation">
                    {(p) => (
                      <Select value={w.query.op ?? ops[0]} onValueChange={(v) => setQ({ op: v as MetricOp })}>
                        <SelectTrigger id={p.id} className="w-full">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {ops.map((o) => (
                            <SelectItem key={o} value={o}>
                              {OP_LABEL[o]}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    )}
                  </Field>
                  <Field label="Group by" hint="Attribute, environment or release">
                    {(p) => <Input {...p} list="group-by-options" className="font-mono" value={w.query.group_by ?? ''} onChange={(e) => setQ({ group_by: e.target.value || undefined })} placeholder="None" />}
                  </Field>
                  <datalist id="group-by-options">
                    {['environment', 'release', ...(meta?.attributes ?? [])].map((a) => (
                      <option key={a} value={a} />
                    ))}
                  </datalist>
                </div>
              </>
            )}
            {w.kind === 'transactions' && (
              <>
                <Field label="Transaction" hint="The exact name, as listed on the Performance page.">
                  {(p) => <Input {...p} className="font-mono" value={w.query.transaction ?? ''} onChange={(e) => setQ({ transaction: e.target.value })} placeholder="POST /api/orders" />}
                </Field>
                <Field label="Value">
                  {(p) => (
                    <Select value={w.query.field ?? 'p95'} onValueChange={(v) => setQ({ field: v as 'p95' })}>
                      <SelectTrigger id={p.id} className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="count">Throughput (count)</SelectItem>
                        <SelectItem value="p50">Duration p50</SelectItem>
                        <SelectItem value="p95">Duration p95</SelectItem>
                      </SelectContent>
                    </Select>
                  )}
                </Field>
              </>
            )}
            {(w.kind === 'logs' || w.kind === 'issues') && (
              <Field label="Query" hint={w.kind === 'logs' ? 'Log search, e.g. level:error service.name:api' : 'Issue search, e.g. is:unresolved level:fatal'}>
                {(p) => <Input {...p} className="font-mono" value={w.query.query ?? ''} onChange={(e) => setQ({ query: e.target.value })} />}
              </Field>
            )}
            {w.kind !== 'issues' && (
              <div className="space-y-1.5">
                <span className="block text-[13px] font-medium">Chart</span>
                <ToggleGroup type="single" value={w.chart} onValueChange={(v) => v && set({ chart: v as ChartType })} aria-label="Chart type" className="h-8">
                  <ToggleGroupItem value="line">Line</ToggleGroupItem>
                  <ToggleGroupItem value="area">Area</ToggleGroupItem>
                  <ToggleGroupItem value="bar">Bar</ToggleGroupItem>
                </ToggleGroup>
              </div>
            )}
            <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-2">
              <Field label="Title">{(p) => <Input {...p} value={w.title} onChange={(e) => set({ title: e.target.value })} placeholder={autoTitle} />}</Field>
              <div className="space-y-1.5">
                <span className="block text-[13px] font-medium">Width</span>
                <ToggleGroup type="single" value={String(w.width)} onValueChange={(v) => v && set({ width: Number(v) })} aria-label="Widget width in columns" className="h-8">
                  {[1, 2, 3, 4].map((n) => (
                    <ToggleGroupItem key={n} value={String(n)} aria-label={`${n} of 4 columns`}>
                      {n}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
              </div>
            </div>
          </div>
          <div className="min-w-0 rounded-lg border bg-card p-3">
            <p className="mb-2 text-[13px] font-semibold">{w.title || autoTitle}</p>
            {valid ? <WidgetView widget={w} scope={{ org, project: projects, environment: environments, period }} height={220} /> : <div className="flex h-[220px] items-center justify-center text-xs text-muted-foreground">Choose what to show to see a preview</div>}
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button disabled={!valid} onClick={() => onSave({ ...w, title: w.title.trim() || autoTitle })}>
            {initial ? 'Save widget' : 'Add widget'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function DashboardPage({ dashboardId }: { dashboardId: string }) {
  const { org, projects, environments, period } = useGlobalFilters()
  const navigate = useNavigate()
  const qc = useQueryClient()
  const q = useQuery({ queryKey: tk.dashboard(org, dashboardId), queryFn: () => telemetryApi.dashboards.get(org, dashboardId) })
  const [editing, setEditing] = useState<{ open: boolean; widget: Widget | null }>({ open: false, widget: null })
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [renaming, setRenaming] = useState<string | null>(null)
  const save = useMutation({
    mutationFn: (patch: Partial<Pick<Dashboard, 'title' | 'description' | 'widgets'>>) => telemetryApi.dashboards.update(org, dashboardId, patch),
    onMutate: async (patch) => {
      await qc.cancelQueries({ queryKey: tk.dashboard(org, dashboardId) })
      const prev = qc.getQueryData<Dashboard>(tk.dashboard(org, dashboardId))
      if (prev) qc.setQueryData(tk.dashboard(org, dashboardId), { ...prev, ...patch })
      return { prev }
    },
    onError: (e, _p, ctx) => {
      if (ctx?.prev) qc.setQueryData(tk.dashboard(org, dashboardId), ctx.prev)
      toast.error('Could not save the dashboard', { description: errorMessage(e) })
    },
    onSettled: () => void qc.invalidateQueries({ queryKey: tk.dashboards(org) }),
  })
  const remove = useMutation({
    mutationFn: () => telemetryApi.dashboards.remove(org, dashboardId),
    onSuccess: () => {
      toast.success('Dashboard deleted')
      void qc.invalidateQueries({ queryKey: tk.dashboards(org) })
      void navigate({ to: '/$org/dashboards', params: { org } })
    },
    onError: (e) => toast.error('Could not delete', { description: errorMessage(e) }),
  })
  const d = q.data
  const widgets = d?.widgets ?? []
  const setWidgets = (next: Widget[]) => save.mutate({ widgets: next })
  const move = (i: number, delta: number) => {
    const next = [...widgets]
    const j = i + delta
    if (j < 0 || j >= next.length) return
    ;[next[i], next[j]] = [next[j], next[i]]
    setWidgets(next)
  }
  const scope = { org, project: projects, environment: environments, period }

  if (q.isError) return <ErrorState error={q.error} retry={() => q.refetch()} className="min-h-[60vh]" />

  return (
    <div>
      <ExploreHeader
        title={d?.title ?? 'Dashboard'}
        description={
          <span className="flex items-center gap-1.5">
            <Link to="/$org/dashboards" params={{ org }} className="hover:text-foreground">
              Dashboards
            </Link>
            {d?.description && (
              <>
                <ChevronRight className="size-3" />
                <span className="truncate">{d.description}</span>
              </>
            )}
          </span>
        }
        actions={
          <>
            <Button size="sm" onClick={() => setEditing({ open: true, widget: null })} disabled={!d}>
              <Plus /> Add widget
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="icon-sm" aria-label="Dashboard actions">
                  <Ellipsis />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => setRenaming(d?.title ?? '')}>
                  <Pencil /> Rename…
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="destructive" onSelect={() => setConfirmDelete(true)}>
                  <Trash2 /> Delete dashboard…
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />
      <div className="p-3 sm:p-5">
        {!d ? (
          <CardsSkeleton count={4} className="md:grid-cols-2" />
        ) : widgets.length === 0 ? (
          <EmptyState icon={<LayoutDashboard />} title="This dashboard is empty" description="Add metric charts, transaction latency, log volume or an issue list." action={<Button size="sm" onClick={() => setEditing({ open: true, widget: null })}><Plus /> Add widget</Button>} />
        ) : (
          <ul className="grid grid-cols-1 gap-3 md:grid-cols-4" aria-label="Widgets">
            {widgets.map((w, i) => (
              <li key={w.id} className={cn('min-w-0 rounded-lg border bg-card', SPAN[w.width] ?? 'md:col-span-2')} aria-label={w.title}>
                <div className="flex items-center gap-2 border-b px-3 py-1.5">
                  <h2 className="min-w-0 flex-1 truncate text-[13px] font-semibold" title={w.title}>
                    {w.title}
                  </h2>
                  <span className="text-[11px] text-muted-foreground">{KIND_LABEL[w.kind]}</span>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="ghost" size="icon-xs" aria-label={`Actions for ${w.title}`}>
                        <Ellipsis />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onSelect={() => setEditing({ open: true, widget: w })}>
                        <Pencil /> Edit
                      </DropdownMenuItem>
                      <DropdownMenuItem onSelect={() => setWidgets([...widgets.slice(0, i + 1), { ...w, id: newId(), title: `${w.title} (copy)` }, ...widgets.slice(i + 1)])}>
                        <Copy /> Duplicate
                      </DropdownMenuItem>
                      <DropdownMenuItem disabled={i === 0} onSelect={() => move(i, -1)}>
                        <ArrowLeft /> Move earlier
                      </DropdownMenuItem>
                      <DropdownMenuItem disabled={i === widgets.length - 1} onSelect={() => move(i, 1)}>
                        <ArrowRight /> Move later
                      </DropdownMenuItem>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem variant="destructive" onSelect={() => setWidgets(widgets.filter((x) => x.id !== w.id))}>
                        <Trash2 /> Remove
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
                <div className="p-3">
                  <WidgetView widget={w} scope={scope} />
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
      <WidgetEditor
        open={editing.open}
        initial={editing.widget}
        onOpenChange={(open) => setEditing((e) => ({ ...e, open }))}
        onSave={(w) => {
          setWidgets(editing.widget ? widgets.map((x) => (x.id === w.id ? w : x)) : [...widgets, w])
          setEditing({ open: false, widget: null })
          toast.success(editing.widget ? 'Widget saved' : 'Widget added')
        }}
      />
      <Dialog open={renaming !== null} onOpenChange={(o) => !o && setRenaming(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Rename dashboard</DialogTitle>
            <DialogDescription>The new title is shown to everyone in the organization.</DialogDescription>
          </DialogHeader>
          <form
            className="space-y-3"
            onSubmit={(e) => {
              e.preventDefault()
              if (renaming?.trim()) save.mutate({ title: renaming.trim() })
              setRenaming(null)
            }}
          >
            <Field label="Title">{(p) => <Input {...p} autoFocus value={renaming ?? ''} onChange={(e) => setRenaming(e.target.value)} />}</Field>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setRenaming(null)}>
                Cancel
              </Button>
              <Button type="submit" disabled={!renaming?.trim()}>
                Rename
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this dashboard?</AlertDialogTitle>
            <AlertDialogDescription>The widgets are removed for everyone in the organization. The data they show is not affected.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction destructive onClick={() => remove.mutate()}>
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
