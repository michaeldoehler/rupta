// TanStack Query keys and hooks. Components use these, never `api` directly
// for reads, so caching and invalidation rules live in one place.

import {
  keepPreviousData,
  queryOptions,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
  type InfiniteData,
  type QueryClient,
} from '@tanstack/react-query'
import { toast } from 'sonner'
import { errorMessage } from './client'
import { api } from './endpoints'
import type { Assignee, Id, Issue, IssueDetail, IssueQueryParams, IssueUpdate, Page, Project } from './types'

export const qk = {
  authConfig: ['auth', 'config'] as const,
  session: ['auth', 'session'] as const,
  user: ['user'] as const,
  userTokens: ['user', 'tokens'] as const,
  invitation: (token: string) => ['invitation', token] as const,

  orgs: ['orgs'] as const,
  org: (org: string) => ['orgs', org] as const,
  members: (org: string) => ['orgs', org, 'members'] as const,
  invites: (org: string) => ['orgs', org, 'invites'] as const,
  teams: (org: string) => ['orgs', org, 'teams'] as const,
  audit: (org: string, filters: object) => ['orgs', org, 'audit', filters] as const,
  sso: (org: string) => ['orgs', org, 'sso'] as const,
  orgTokens: (org: string) => ['orgs', org, 'tokens'] as const,
  channels: (org: string) => ['orgs', org, 'channels'] as const,
  environments: (org: string, projects: string[]) => ['orgs', org, 'environments', projects] as const,
  tagKeys: (org: string, projects: string[]) => ['orgs', org, 'tag-keys', projects] as const,
  tagValues: (org: string, key: string, projects: string[], query: string) => ['orgs', org, 'tag-values', key, projects, query] as const,
  savedSearches: (org: string) => ['orgs', org, 'saved-searches'] as const,

  projects: (org: string) => ['orgs', org, 'projects'] as const,
  project: (org: string, project: string) => ['projects', org, project] as const,
  keys: (org: string, project: string) => ['projects', org, project, 'keys'] as const,
  bundles: (org: string, project: string) => ['projects', org, project, 'bundles'] as const,
  alertRules: (org: string, project: string) => ['projects', org, project, 'alert-rules'] as const,
  alertRule: (org: string, project: string, id: Id) => ['projects', org, project, 'alert-rules', id] as const,
  feedback: (org: string, project: string, query: string) => ['projects', org, project, 'feedback', query] as const,
  monitors: (org: string, project: string) => ['projects', org, project, 'monitors'] as const,
  monitor: (org: string, project: string, slug: string) => ['projects', org, project, 'monitors', slug] as const,
  checkins: (org: string, project: string, slug: string) => ['projects', org, project, 'monitors', slug, 'checkins'] as const,

  issues: {
    all: ['issues'] as const,
    lists: () => ['issues', 'list'] as const,
    list: (org: string, params: IssueQueryParams) => ['issues', 'list', org, params] as const,
    detail: (id: Id) => ['issues', 'detail', id] as const,
    event: (id: Id, which: string) => ['issues', 'detail', id, 'event', which] as const,
    events: (id: Id, query: string) => ['issues', 'detail', id, 'events', query] as const,
    tags: (id: Id) => ['issues', 'detail', id, 'tags'] as const,
    tagValues: (id: Id, key: string) => ['issues', 'detail', id, 'tags', key] as const,
    activity: (id: Id) => ['issues', 'detail', id, 'activity'] as const,
    stats: (id: Id, period: string) => ['issues', 'detail', id, 'stats', period] as const,
    feedback: (id: Id) => ['issues', 'detail', id, 'feedback'] as const,
    attachments: (id: Id) => ['issues', 'detail', id, 'attachments'] as const,
  },

  releases: (org: string, params: object) => ['releases', org, params] as const,
  release: (org: string, version: string) => ['releases', org, 'detail', version] as const,
}

// ---- auth --------------------------------------------------------------------

export const authConfigQuery = queryOptions({ queryKey: qk.authConfig, queryFn: api.auth.config, staleTime: 60_000 })
export const sessionQuery = queryOptions({
  queryKey: qk.session,
  queryFn: async () => {
    try {
      return await api.auth.session()
    } catch (e) {
      if ((e as { status?: number }).status === 401) return null
      throw e
    }
  },
  staleTime: 5 * 60_000,
})

export function useSession() {
  return useQuery(sessionQuery)
}

// ---- org-level data ----------------------------------------------------------

export const projectsQuery = (org: string) => queryOptions({ queryKey: qk.projects(org), queryFn: () => api.projects.list(org), staleTime: 60_000 })

export function useProjects(org: string) {
  return useQuery(projectsQuery(org))
}

export function useMembers(org: string) {
  return useQuery({ queryKey: qk.members(org), queryFn: () => api.orgs.members(org), staleTime: 60_000 })
}

export function useTeams(org: string) {
  return useQuery({ queryKey: qk.teams(org), queryFn: () => api.orgs.teams(org), staleTime: 60_000 })
}

export function useEnvironments(org: string, projects: string[]) {
  return useQuery({ queryKey: qk.environments(org, projects), queryFn: () => api.orgs.environments(org, projects), staleTime: 5 * 60_000 })
}

export function useTagKeys(org: string, projects: string[]) {
  return useQuery({ queryKey: qk.tagKeys(org, projects), queryFn: () => api.orgs.tagKeys(org, projects), staleTime: 5 * 60_000 })
}

export function useTagValues(org: string, key: string | null, projects: string[], query: string) {
  return useQuery({
    queryKey: qk.tagValues(org, key ?? '', projects, query),
    queryFn: () => api.orgs.tagValues(org, key!, { project: projects, query }),
    enabled: Boolean(key),
    staleTime: 60_000,
    placeholderData: keepPreviousData,
  })
}

export function useSavedSearches(org: string) {
  return useQuery({ queryKey: qk.savedSearches(org), queryFn: () => api.orgs.savedSearches(org), staleTime: 60_000 })
}

// ---- issues ------------------------------------------------------------------

export const ISSUE_PAGE_SIZE = 100

export function useIssueList(org: string, params: IssueQueryParams, opts: { refetchInterval?: number | false } = {}) {
  return useInfiniteQuery({
    queryKey: qk.issues.list(org, params),
    queryFn: ({ pageParam, signal }) => api.issues.list(org, { ...params, cursor: pageParam ?? undefined, limit: ISSUE_PAGE_SIZE }, signal),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next_cursor,
    placeholderData: keepPreviousData,
    refetchInterval: opts.refetchInterval,
    staleTime: 15_000,
  })
}

export const issueQuery = (id: Id) => queryOptions({ queryKey: qk.issues.detail(id), queryFn: () => api.issues.get(id), staleTime: 15_000 })

export function useIssue(id: Id) {
  return useQuery(issueQuery(id))
}

export const eventQuery = (id: Id, which: string) =>
  queryOptions({ queryKey: qk.issues.event(id, which), queryFn: () => api.issues.event(id, which), staleTime: which.length > 20 ? Infinity : 30_000 })

type IssueListData = InfiniteData<Page<Issue>, string | null>

/** Applies an update to an issue object as the server would, for optimistic UI. */
export function patchIssue<T extends Issue | IssueDetail>(issue: T, update: IssueUpdate, assignee?: Assignee | null): T {
  const next = { ...issue }
  if (update.status) {
    next.status = update.status
    next.substatus = update.status === 'resolved' ? null : update.status === 'ignored' ? 'archived_until_escalating' : 'ongoing'
  }
  if (update.assignee !== undefined) next.assignee = update.assignee ? (assignee ?? { ...update.assignee, name: '…' }) : null
  if (update.is_bookmarked !== undefined) next.is_bookmarked = update.is_bookmarked
  if (update.has_seen !== undefined) next.has_seen = update.has_seen
  if (update.priority) next.priority = update.priority
  return next
}

function snapshotIssues(qc: QueryClient) {
  return {
    lists: qc.getQueriesData<IssueListData>({ queryKey: qk.issues.lists() }),
    details: qc.getQueriesData<IssueDetail>({ queryKey: ['issues', 'detail'], predicate: (q) => q.queryKey.length === 3 }),
  }
}

function writeIssues(qc: QueryClient, ids: Set<Id>, fn: <T extends Issue | IssueDetail>(i: T) => T | null) {
  qc.setQueriesData<IssueListData>({ queryKey: qk.issues.lists() }, (data) =>
    data
      ? {
          ...data,
          pages: data.pages.map((pg) => ({
            ...pg,
            data: pg.data.flatMap((i) => {
              if (!ids.has(i.id)) return [i]
              const r = fn(i)
              return r ? [r] : []
            }),
          })),
        }
      : data,
  )
  for (const id of ids) {
    qc.setQueryData<IssueDetail>(qk.issues.detail(id), (d) => (d ? (fn(d) ?? d) : d))
  }
}

export interface IssueMutation {
  ids: Id[]
  update: IssueUpdate
  /** Display value for optimistic assignee updates. */
  assignee?: Assignee | null
  /** Shown in the success toast, e.g. "Resolved 3 issues". */
  message?: string
  /** Offer an undo with this inverse update. */
  undo?: IssueUpdate
}

/** Bulk/single issue update with optimistic cache writes and rollback. */
export function useUpdateIssues(org: string) {
  const qc = useQueryClient()
  const mutation = useMutation({
    mutationFn: (m: IssueMutation) =>
      m.ids.length === 1 ? api.issues.update(m.ids[0], m.update).then(() => undefined) : api.issues.bulkUpdate(org, m.ids, m.update).then(() => undefined),
    onMutate: async (m) => {
      await qc.cancelQueries({ queryKey: qk.issues.all })
      const snapshot = snapshotIssues(qc)
      writeIssues(qc, new Set(m.ids), (i) => patchIssue(i, m.update, m.assignee))
      return { snapshot }
    },
    onError: (e, _m, ctx) => {
      ctx?.snapshot.lists.forEach(([key, data]) => qc.setQueryData(key, data))
      ctx?.snapshot.details.forEach(([key, data]) => qc.setQueryData(key, data))
      toast.error('Could not update the issue', { description: errorMessage(e) })
    },
    onSuccess: (_d, m) => {
      if (m.message) {
        toast.success(m.message, m.undo ? { action: { label: 'Undo', onClick: () => mutation.mutate({ ids: m.ids, update: m.undo! }) } } : undefined)
      }
    },
    onSettled: (_d, _e, m) => {
      void qc.invalidateQueries({ queryKey: qk.issues.lists() })
      m.ids.forEach((id) => {
        void qc.invalidateQueries({ queryKey: qk.issues.detail(id), exact: true })
        void qc.invalidateQueries({ queryKey: qk.issues.activity(id) })
      })
    },
  })
  return mutation
}

export function useDeleteIssues(org: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (ids: Id[]) => api.issues.bulkDelete(org, ids),
    onMutate: async (ids) => {
      await qc.cancelQueries({ queryKey: qk.issues.lists() })
      const snapshot = snapshotIssues(qc)
      writeIssues(qc, new Set(ids), () => null)
      return { snapshot }
    },
    onError: (e, _ids, ctx) => {
      ctx?.snapshot.lists.forEach(([key, data]) => qc.setQueryData(key, data))
      toast.error('Could not delete', { description: errorMessage(e) })
    },
    onSuccess: (_d, ids) => toast.success(ids.length === 1 ? 'Issue deleted' : `${ids.length} issues deleted`),
    onSettled: () => qc.invalidateQueries({ queryKey: qk.issues.lists() }),
  })
}

// ---- generic mutation helper -------------------------------------------------

/** Mutation that toasts on error/success and invalidates the given keys. */
export function useApiMutation<TVars, TData = unknown>(
  fn: (vars: TVars) => Promise<TData>,
  opts: {
    invalidate?: readonly (readonly unknown[])[]
    success?: string | ((d: TData, v: TVars) => string)
    onSuccess?: (d: TData, v: TVars) => void
    silentError?: boolean
  } = {},
) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: fn,
    onSuccess: (d, v) => {
      if (opts.success) toast.success(typeof opts.success === 'function' ? opts.success(d, v) : opts.success)
      opts.onSuccess?.(d, v)
    },
    onError: (e) => {
      if (!opts.silentError) toast.error(errorMessage(e))
    },
    onSettled: () => {
      opts.invalidate?.forEach((key) => void qc.invalidateQueries({ queryKey: key }))
    },
  })
}

export function projectBySlug(projects: Project[] | undefined, slug: string) {
  return projects?.find((p) => p.slug === slug)
}
