import { QueryClient } from '@tanstack/react-query'
import { createRouter } from '@tanstack/react-router'
import { ApiError } from '@/api/client'
import { RouteError, RouteNotFound, RoutePending } from '@/components/route-states'
import { routeTree } from './routeTree.gen'

export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: (count, error) => {
          if (error instanceof ApiError && error.status >= 400 && error.status < 500) return false
          return count < 2
        },
        refetchOnWindowFocus: true,
        staleTime: 10_000,
      },
      mutations: { retry: false },
    },
  })
}

export function createAppRouter(queryClient: QueryClient) {
  return createRouter({
    routeTree,
    context: { queryClient },
    defaultPreload: 'intent',
    defaultPreloadStaleTime: 0,
    defaultPendingMs: 150,
    defaultPendingMinMs: 0,
    defaultErrorComponent: RouteError,
    defaultNotFoundComponent: RouteNotFound,
    defaultPendingComponent: RoutePending,
    scrollRestoration: true,
  })
}

declare module '@tanstack/react-router' {
  interface Register {
    router: ReturnType<typeof createAppRouter>
  }
}
