import { useQuery, useQueryClient, type InfiniteData } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import { ArrowDownUp, ArrowUp, Bookmark, BookmarkCheck, ChevronDown, CircleCheckBig, Inbox, Save, Star, Trash2, UserPlus } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api } from '@/api/endpoints'
import { qk, useApiMutation, useDeleteIssues, useIssueList, useSavedSearches, useSession, useUpdateIssues } from '@/api/queries'
import type { Assignee, Issue, IssueQueryParams, IssueSort, IssueUpdate, Page } from '@/api/types'
import { ErrorState, EmptyState, ListSkeleton } from '@/components/common/states'
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
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Field } from '@/components/common/field'
import { useDocumentVisible } from '@/hooks/use-document-visible'
import { useHotkeys } from '@/hooks/use-hotkeys'
import { thousands } from '@/lib/format'
import { formatFilter, tokenize } from '@/lib/search/parse'
import { cn } from '@/lib/utils'
import { FilterBar } from '@/features/shell/filter-bar'
import { useGlobalFilters } from '@/features/shell/use-global-filters'
import { AssigneePicker, IgnoreButton, ResolveButton, type ActionHandler } from './actions'
import { IssueRow, ROW_HEIGHT } from './issue-row'
import { SearchBar, type SearchBarHandle } from './search-bar'

export const DEFAULT_QUERY = 'is:unresolved'

export const TABS: { id: string; label: string; is: string | null; hint: string }[] = [
  { id: 'unresolved', label: 'Unresolved', is: 'unresolved', hint: 'All open issues' },
  { id: 'for_review', label: 'For review', is: 'new', hint: 'New issues that have not been triaged yet' },
  { id: 'regressed', label: 'Regressed', is: 'regressed', hint: 'Resolved issues that came back' },
  { id: 'escalating', label: 'Escalating', is: 'escalating', hint: 'Issues with sharply rising event volume' },
  { id: 'ignored', label: 'Archived', is: 'ignored', hint: 'Ignored issues' },
  { id: 'resolved', label: 'Resolved', is: 'resolved', hint: 'Issues marked as fixed' },
  { id: 'all', label: 'All', is: null, hint: 'Every issue' },
]

const STATUS_VALUES = new Set(['unresolved', 'resolved', 'ignored', 'archived', 'new', 'ongoing', 'regressed', 'escalating'])

export function currentTab(query: string): string {
  const statusTokens = tokenize(query).filter(
    (t) => t.type === 'filter' && t.key === 'is' && !t.negated && t.values.length === 1 && STATUS_VALUES.has(t.values[0]),
  )
  if (statusTokens.length === 0) return 'all'
  if (statusTokens.length > 1) return ''
  const v = statusTokens[0].type === 'filter' ? statusTokens[0].values[0] : ''
  return TABS.find((t) => t.is === v)?.id ?? (v === 'archived' ? 'ignored' : '')
}

/** Replaces the status part of the query with the tab's `is:` filter. */
export function queryForTab(query: string, tabId: string): string {
  const tab = TABS.find((t) => t.id === tabId)
  const kept = tokenize(query)
    .filter((t) => !(t.type === 'filter' && t.key === 'is' && !t.negated && t.values.every((v) => STATUS_VALUES.has(v))))
    .map((t) => t.raw)
  if (tab?.is) kept.unshift(formatFilter('is', tab.is))
  return kept.join(' ')
}

const SORTS: { id: IssueSort; label: string }[] = [
  { id: 'last_seen', label: 'Last seen' },
  { id: 'first_seen', label: 'First seen' },
  { id: 'freq', label: 'Events' },
  { id: 'users', label: 'Users' },
  { id: 'priority', label: 'Priority' },
]

type ListData = InfiniteData<Page<Issue> & { total?: number }, string | null>

export function IssueStreamPage({ query, sort }: { query: string; sort: IssueSort }) {
  const { org, projects, environments, period, update: updateGlobal } = useGlobalFilters()
  // The period limits the stream to issues seen in it and sets the sparkline range.
  const stats = period
  const navigate = useNavigate()
  const qc = useQueryClient()
  const { data: session } = useSession()
  const visible = useDocumentVisible()
  const searchRef = useRef<SearchBarHandle>(null)
  const scrollRef = useRef<HTMLDivElement>(null)

  const params: IssueQueryParams = useMemo(
    () => ({ project: projects, environment: environments, query, sort, period }),
    [projects, environments, query, sort, period],
  )
  const list = useIssueList(org, params)
  const issues = useMemo(() => list.data?.pages.flatMap((p) => p.data) ?? [], [list.data])
  const total = (list.data?.pages[0] as { total?: number } | undefined)?.total

  // ---- live updates: poll the first page; merge counts in place and offer new issues.
  const probe = useQuery({
    queryKey: ['issues', 'probe', org, params],
    queryFn: ({ signal }) => api.issues.list(org, { ...params, limit: 50 }, signal),
    refetchInterval: visible ? 30_000 : false,
    refetchIntervalInBackground: false,
    enabled: list.isSuccess,
    staleTime: 25_000,
  })
  const shownIds = useMemo(() => new Set(issues.map((i) => i.id)), [issues])
  const newIssues = useMemo(() => (probe.data ? probe.data.data.filter((i) => !shownIds.has(i.id)) : []), [probe.data, shownIds])
  useEffect(() => {
    if (!probe.data) return
    const fresh = new Map(probe.data.data.map((i) => [i.id, i]))
    qc.setQueryData<ListData>(qk.issues.list(org, params), (d) =>
      d
        ? {
            ...d,
            pages: d.pages.map((pg) => ({
              ...pg,
              data: pg.data.map((i) => {
                const f = fresh.get(i.id)
                return f && (f.count !== i.count || f.last_seen !== i.last_seen)
                  ? { ...i, count: f.count, user_count: f.user_count, last_seen: f.last_seen, stats: f.stats }
                  : i
              }),
            })),
          }
        : d,
    )
  }, [probe.data, qc, org, params])

  // ---- selection & focus
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [focusedId, setFocusedId] = useState<string | null>(null)
  const anchor = useRef<string | null>(null)
  useEffect(() => {
    setSelected(new Set())
    setFocusedId(null)
  }, [org, query, period, sort, projects, environments])

  const toggle = useCallback(
    (id: string, shift: boolean) => {
      setSelected((prev) => {
        const next = new Set(prev)
        if (shift && anchor.current) {
          const a = issues.findIndex((i) => i.id === anchor.current)
          const b = issues.findIndex((i) => i.id === id)
          if (a >= 0 && b >= 0) {
            const [lo, hi] = a < b ? [a, b] : [b, a]
            for (let k = lo; k <= hi; k++) next.add(issues[k].id)
            return next
          }
        }
        if (next.has(id)) next.delete(id)
        else next.add(id)
        anchor.current = id
        return next
      })
    },
    [issues],
  )

  const allSelected = issues.length > 0 && selected.size === issues.length
  const someSelected = selected.size > 0 && !allSelected

  // ---- actions
  const update = useUpdateIssues(org)
  const del = useDeleteIssues(org)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const targetIds = useCallback(() => (selected.size ? [...selected] : focusedId ? [focusedId] : []), [selected, focusedId])

  const act: ActionHandler = useCallback(
    (u, message, opts) => {
      const ids = targetIds()
      if (!ids.length) return
      update.mutate({ ids, update: u, message, assignee: opts?.assignee, undo: opts?.undo })
    },
    [targetIds, update],
  )

  const assignOne = useCallback(
    (issue: Issue, a: Assignee | null) =>
      update.mutate({
        ids: [issue.id],
        update: { assignee: a ? { type: a.type, id: a.id } : null },
        assignee: a,
        message: a ? `Assigned ${issue.short_id} to ${a.name}` : `${issue.short_id} unassigned`,
      }),
    [update],
  )

  const me: Assignee | null = session ? { type: 'user', id: session.user.id, name: session.user.name || session.user.email } : null
  const selectionIssues = issues.filter((i) => selected.has(i.id))
  const allBookmarked = selectionIssues.length > 0 && selectionIssues.every((i) => i.is_bookmarked)

  // ---- virtualisation
  const rowCount = issues.length + (list.hasNextPage ? 1 : 0)
  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    // Used until the scroll container is measured (and in tests without layout).
    initialRect: { width: 1200, height: 900 },
    overscan: 12,
    getItemKey: (i) => issues[i]?.id ?? `loader-${i}`,
  })
  const items = virtualizer.getVirtualItems()
  const lastItem = items[items.length - 1]
  useEffect(() => {
    if (lastItem && lastItem.index >= issues.length - 20 && list.hasNextPage && !list.isFetchingNextPage) void list.fetchNextPage()
  }, [lastItem, issues.length, list])

  const move = useCallback(
    (delta: number) => {
      if (!issues.length) return
      const idx = focusedId ? issues.findIndex((i) => i.id === focusedId) : -1
      const next = Math.max(0, Math.min(issues.length - 1, idx === -1 ? 0 : idx + delta))
      setFocusedId(issues[next].id)
      virtualizer.scrollToIndex(next, { align: 'auto' })
    },
    [issues, focusedId, virtualizer],
  )

  const focused = issues.find((i) => i.id === focusedId)
  useHotkeys(
    {
      j: () => move(1),
      k: () => move(-1),
      arrowdown: () => move(1),
      x: () => focusedId && toggle(focusedId, false),
      'shift+x': () => focusedId && toggle(focusedId, true),
      o: () => focused && void navigate({ to: '/$org/issues/$issueId', params: { org, issueId: focused.id } }),
      Enter: () => focused && void navigate({ to: '/$org/issues/$issueId', params: { org, issueId: focused.id } }),
      e: () => act({ status: 'resolved' }, selected.size > 1 ? `${selected.size} issues resolved` : 'Issue resolved', { undo: { status: 'unresolved' } }),
      i: () =>
        act({ status: 'ignored', substatus: 'archived_until_escalating' }, selected.size > 1 ? `${selected.size} issues ignored` : 'Issue ignored', {
          undo: { status: 'unresolved' },
        }),
      u: () => act({ status: 'unresolved' }, 'Marked unresolved'),
      a: () => me && act({ assignee: { type: 'user', id: me.id } }, 'Assigned to you', { assignee: me }),
      b: () => {
        const ids = targetIds()
        const on = !issues.filter((i) => ids.includes(i.id)).every((i) => i.is_bookmarked)
        act({ is_bookmarked: on }, on ? 'Bookmarked' : 'Bookmark removed')
      },
      '/': () => searchRef.current?.focus(),
      Escape: () => {
        setSelected(new Set())
        setFocusedId(null)
      },
    },
    [move, toggle, focusedId, focused, act, selected, me, issues, targetIds],
  )

  // ---- search params
  const setSearch = (patch: { query?: string; sort?: IssueSort }) =>
    void navigate({
      to: '/$org/issues',
      params: { org },
      search: (prev: Record<string, unknown>) => {
        const next = { ...prev, ...patch }
        return {
          ...next,
          query: next.query === DEFAULT_QUERY ? undefined : next.query,
          sort: next.sort === 'last_seen' ? undefined : next.sort,
        }
      },
    })
  const tab = currentTab(query)

  const showNew = () => {
    void list.refetch()
    scrollRef.current?.scrollTo({ top: 0 })
  }

  return (
    <div className="flex h-full flex-col">
      <div className="space-y-3 border-b px-3 pt-3 pb-0 sm:px-5">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-3">
            <h1 className="text-lg font-semibold tracking-tight">Issues</h1>
            <SavedSearches org={org} query={query} sort={sort} onPick={(q, s) => setSearch({ query: q, sort: s })} />
          </div>
          <FilterBar />
        </div>
        <SearchBar ref={searchRef} org={org} projects={projects} value={query} onSubmit={(q) => setSearch({ query: q })} />
        <nav aria-label="Issue status" className="-mb-px flex gap-1 overflow-x-auto">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              title={t.hint}
              onClick={() => setSearch({ query: queryForTab(query, t.id) })}
              aria-current={tab === t.id ? 'page' : undefined}
              className={cn(
                'shrink-0 border-b-2 border-transparent px-2.5 pt-1 pb-2 text-[13px] font-medium text-muted-foreground transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none',
                tab === t.id && 'border-brand text-foreground',
              )}
            >
              {t.label}
            </button>
          ))}
        </nav>
      </div>

      {/* toolbar */}
      <div className="flex min-h-11 flex-wrap items-center gap-2 border-b bg-muted/30 px-3 py-1.5 sm:px-5" role="toolbar" aria-label="Bulk actions">
        <div className="flex items-center gap-2 pl-[13px]">
          <Checkbox
            checked={allSelected ? true : someSelected ? 'indeterminate' : false}
            onCheckedChange={() => setSelected(allSelected || someSelected ? new Set() : new Set(issues.map((i) => i.id)))}
            aria-label={allSelected ? 'Deselect all' : 'Select all loaded issues'}
            disabled={!issues.length}
          />
        </div>
        {selected.size > 0 ? (
          <>
            <span className="text-[13px] font-medium tabular">{selected.size} selected</span>
            <ResolveButton resolved={false} onAction={act} count={selected.size} currentRelease={null} />
            <IgnoreButton ignored={false} onAction={act} count={selected.size} />
            <AssigneePicker
              org={org}
              value={null}
              align="start"
              onChange={(a) =>
                act(
                  { assignee: a ? { type: a.type, id: a.id } : null },
                  a ? `${selected.size} issues assigned to ${a.name}` : `${selected.size} issues unassigned`,
                  { assignee: a },
                )
              }
              trigger={
                <Button variant="outline" size="sm">
                  <UserPlus /> Assign
                </Button>
              }
            />
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="sm" aria-label="More actions">
                  More <ChevronDown />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuItem onSelect={() => act({ is_bookmarked: !allBookmarked }, allBookmarked ? 'Bookmarks removed' : 'Bookmarked')}>
                  {allBookmarked ? <BookmarkCheck /> : <Bookmark />} {allBookmarked ? 'Remove bookmark' : 'Bookmark'}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => act({ status: 'unresolved' }, 'Marked unresolved')}>
                  <Inbox /> Mark unresolved
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => act({ has_seen: true }, 'Marked as reviewed')}>
                  <CircleCheckBig /> Mark reviewed
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="destructive" onSelect={() => setConfirmDelete(true)}>
                  <Trash2 /> Delete…
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Button variant="ghost" size="sm" onClick={() => setSelected(new Set())}>
              Clear
            </Button>
          </>
        ) : (
          <span className="text-[13px] text-muted-foreground tabular">
            {list.isPending ? 'Loading…' : total !== undefined ? `${thousands(total)} issues` : `${issues.length}${list.hasNextPage ? '+' : ''} issues`}
          </span>
        )}
        <div className="ml-auto flex items-center gap-2">
          <ToggleGroup
            type="single"
            value={stats}
            onValueChange={(v) => v && updateGlobal({ period: v as '24h' | '14d' })}
            aria-label="Graph period"
            className="hidden md:inline-flex"
          >
            <ToggleGroupItem value="24h">24h</ToggleGroupItem>
            <ToggleGroupItem value="14d">14d</ToggleGroupItem>
          </ToggleGroup>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm" className="font-normal">
                <ArrowDownUp className="opacity-70" /> <span className="hidden sm:inline">Sort:</span> {SORTS.find((s) => s.id === sort)?.label}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuLabel>Sort by</DropdownMenuLabel>
              <DropdownMenuRadioGroup value={sort} onValueChange={(v) => setSearch({ sort: v as IssueSort })}>
                {SORTS.map((s) => (
                  <DropdownMenuRadioItem key={s.id} value={s.id}>
                    {s.label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      {/* column header */}
      <div
        className="hidden h-7 shrink-0 items-center gap-3 border-b pr-4 pl-[54px] text-[11px] font-medium tracking-wide text-muted-foreground uppercase sm:flex"
        aria-hidden
      >
        <span className="flex-1">Issue</span>
        <span className="hidden w-24 text-right lg:block">Last seen</span>
        <span className="hidden w-[100px] md:block">Graph · {stats}</span>
        <span className="w-16 text-right">Events</span>
        <span className="w-14 text-right">Users</span>
        <span className="w-8" />
      </div>

      {newIssues.length > 0 && (
        <button
          type="button"
          onClick={showNew}
          className="flex items-center justify-center gap-2 border-b bg-brand-soft py-1.5 text-[13px] font-medium text-brand-text hover:brightness-[0.98]"
        >
          <ArrowUp className="size-3.5" /> {newIssues.length === 50 ? '50+' : newIssues.length} new {newIssues.length === 1 ? 'issue' : 'issues'} · Show
        </button>
      )}

      <div
        ref={scrollRef}
        className="relative min-h-0 flex-1 overflow-y-auto contain-strict"
        role={issues.length ? 'grid' : 'region'}
        aria-label="Issues"
        aria-rowcount={issues.length ? (total ?? rowCount) : undefined}
        aria-busy={list.isFetching}
      >
        {list.isPending ? (
          <ListSkeleton rows={12} />
        ) : list.isError ? (
          <ErrorState error={list.error} retry={() => list.refetch()} />
        ) : issues.length === 0 ? (
          <StreamEmpty org={org} tab={tab} query={query} onReset={() => setSearch({ query: DEFAULT_QUERY })} />
        ) : (
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {items.map((vi) => {
              const issue = issues[vi.index]
              return (
                <div key={vi.key} style={{ position: 'absolute', top: 0, left: 0, right: 0, height: vi.size, transform: `translateY(${vi.start}px)` }}>
                  {issue ? (
                    <IssueRow
                      issue={issue}
                      org={org}
                      selected={selected.has(issue.id)}
                      focused={issue.id === focusedId}
                      onToggle={toggle}
                      onFocus={setFocusedId}
                      onAssign={assignOne}
                      statsPeriod={stats}
                      rowIndex={vi.index + 1}
                    />
                  ) : (
                    <div className="flex h-full items-center justify-center text-xs text-muted-foreground">Loading more…</div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>

      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {selected.size === 1 ? 'this issue' : `${selected.size} issues`}?</AlertDialogTitle>
            <AlertDialogDescription>
              Their events are deleted too. If the error happens again, a new issue is created. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              destructive
              onClick={() => {
                del.mutate([...selected])
                setSelected(new Set())
              }}
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

function StreamEmpty({ org, tab, query, onReset }: { org: string; tab: string; query: string; onReset: () => void }) {
  if (tab === 'unresolved' && query.trim() === DEFAULT_QUERY)
    return (
      <EmptyState
        icon={<CircleCheckBig className="text-success" />}
        title="No unresolved issues"
        description="Nothing is broken in the selected projects and period. New errors show up here within seconds of being sent."
        action={
          <Button asChild variant="outline" size="sm">
            <Link to="/$org/settings/projects" params={{ org }}>
              Set up an SDK
            </Link>
          </Button>
        }
      />
    )
  return (
    <EmptyState
      icon={<Inbox />}
      title="No issues match"
      description="Try a different search, another tab, or widen the period and projects at the top."
      action={
        <Button variant="outline" size="sm" onClick={onReset}>
          Reset search
        </Button>
      }
    />
  )
}

function SavedSearches({ org, query, sort, onPick }: { org: string; query: string; sort: IssueSort; onPick: (q: string, s: IssueSort) => void }) {
  const saved = useSavedSearches(org)
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [shared, setShared] = useState(false)
  const create = useApiMutation((v: { name: string; is_shared: boolean }) => api.orgs.createSavedSearch(org, { ...v, query, sort }), {
    invalidate: [qk.savedSearches(org)],
    success: 'Search saved',
    onSuccess: () => {
      setOpen(false)
      setName('')
    },
  })
  const remove = useApiMutation((id: string) => api.orgs.deleteSavedSearch(org, id), { invalidate: [qk.savedSearches(org)], success: 'Saved search deleted' })
  const current = saved.data?.find((s) => s.query === query)
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="sm" className="text-muted-foreground">
            <Star className={cn(current && 'fill-brand text-brand')} /> <span className="max-w-40 truncate">{current?.name ?? 'Saved searches'}</span>{' '}
            <ChevronDown />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-80">
          <DropdownMenuLabel>Saved searches</DropdownMenuLabel>
          {saved.data?.length === 0 && <p className="px-2 py-2 text-xs text-muted-foreground">Nothing saved yet.</p>}
          {saved.data?.map((s) => (
            <DropdownMenuItem key={s.id} onSelect={() => onPick(s.query, s.sort)} className="group items-start">
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5">
                  <span className="truncate font-medium">{s.name}</span>
                  {s.is_shared && <span className="text-[10px] text-muted-foreground uppercase">shared</span>}
                </div>
                <div className="truncate font-mono text-[11px] text-muted-foreground">{s.query}</div>
              </div>
              <button
                type="button"
                className="rounded p-0.5 opacity-0 group-hover:opacity-100 hover:text-destructive focus-visible:opacity-100"
                aria-label={`Delete saved search ${s.name}`}
                onClick={(e) => {
                  e.stopPropagation()
                  e.preventDefault()
                  remove.mutate(s.id)
                }}
              >
                <Trash2 className="size-3.5" />
              </button>
            </DropdownMenuItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => setOpen(true)} disabled={!query.trim()}>
            <Save /> Save current search…
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Save search</DialogTitle>
            <DialogDescription className="font-mono text-xs">{query}</DialogDescription>
          </DialogHeader>
          <form
            className="grid gap-4"
            onSubmit={(e) => {
              e.preventDefault()
              if (name.trim()) create.mutate({ name: name.trim(), is_shared: shared })
            }}
          >
            <Field label="Name">
              {(p) => <Input {...p} autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Checkout errors" />}
            </Field>
            <div className="flex items-center gap-2">
              <Switch id="share-search" checked={shared} onCheckedChange={setShared} />
              <Label htmlFor="share-search" className="font-normal">
                Share with everyone in the organization
              </Label>
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setOpen(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={!name.trim() || create.isPending}>
                Save
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  )
}

export type { IssueUpdate }
