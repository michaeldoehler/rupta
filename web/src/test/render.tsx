import { QueryClientProvider } from '@tanstack/react-query'
import { createMemoryHistory, createRootRoute, createRouter, Outlet, RouterProvider } from '@tanstack/react-router'
import { render } from '@testing-library/react'
import type { ReactNode } from 'react'
import { ThemeProvider } from '@/components/theme-provider'
import { Toaster } from '@/components/ui/sonner'
import { TooltipProvider } from '@/components/ui/tooltip'
import { createQueryClient } from '@/router'
import { routeTree } from '@/routeTree.gen'

function Providers({ children, client }: { children: ReactNode; client: ReturnType<typeof createQueryClient> }) {
  return (
    <ThemeProvider>
      <QueryClientProvider client={client}>
        <TooltipProvider delayDuration={0}>
          {children}
          <Toaster />
        </TooltipProvider>
      </QueryClientProvider>
    </ThemeProvider>
  )
}

/** Renders a component inside a minimal router (for components that use Link). */
export function renderUi(ui: ReactNode) {
  const client = createQueryClient()
  const root = createRootRoute({
    component: () => (
      <>
        {ui}
        <Outlet />
      </>
    ),
  })
  const router = createRouter({ routeTree: root, history: createMemoryHistory({ initialEntries: ['/'] }) })
  const result = render(
    <Providers client={client}>
      <RouterProvider router={router} />
    </Providers>,
  )
  return { ...result, client, router }
}

/** Renders the whole app at `path`, backed by the MSW mock API. */
export function renderApp(path: string) {
  const client = createQueryClient()
  client.setDefaultOptions({ queries: { retry: false, refetchOnWindowFocus: false } })
  const router = createRouter({ routeTree, context: { queryClient: client }, history: createMemoryHistory({ initialEntries: [path] }), defaultPendingMs: 0 })
  const result = render(
    <Providers client={client}>
      <RouterProvider router={router} />
    </Providers>,
  )
  return { ...result, client, router }
}
