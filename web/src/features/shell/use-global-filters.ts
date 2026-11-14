import { getRouteApi, useNavigate } from '@tanstack/react-router'
import { useCallback, useMemo } from 'react'
import { DEFAULT_PERIOD, joinList, splitList, type Period } from '@/lib/search-params'

const orgRoute = getRouteApi('/_auth/$org')

/** The org slug and the global project/environment/period selectors. */
export function useGlobalFilters() {
  const { org } = orgRoute.useParams()
  const search = orgRoute.useSearch()
  const navigate = useNavigate()
  const projects = useMemo(() => splitList(search.project), [search.project])
  const environments = useMemo(() => splitList(search.environment), [search.environment])
  const period: Period = search.period ?? DEFAULT_PERIOD

  const update = useCallback(
    (patch: { project?: string[]; environment?: string[]; period?: Period }) => {
      void navigate({
        to: '.',
        search: (prev: Record<string, unknown>) => ({
          ...prev,
          ...('project' in patch ? { project: joinList(patch.project ?? []) } : {}),
          ...('environment' in patch ? { environment: joinList(patch.environment ?? []) } : {}),
          ...('period' in patch ? { period: patch.period === DEFAULT_PERIOD ? undefined : patch.period } : {}),
          cursor: undefined,
        }),
        replace: true,
      })
    },
    [navigate],
  )

  return { org, projects, environments, period, update }
}

export function useOrgSlug() {
  return orgRoute.useParams().org
}
