import { QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { onUnauthorized } from '@/api/client'
import { qk } from '@/api/queries'
import { ThemeProvider } from '@/components/theme-provider'
import { Toaster } from '@/components/ui/sonner'
import { createAppRouter, createQueryClient } from './router'

export function App() {
  const [queryClient] = useState(createQueryClient)
  const [router] = useState(() => createAppRouter(queryClient))

  useEffect(() => {
    const off = onUnauthorized(() => {
      // The session expired or was revoked: drop it and go to the login page.
      queryClient.setQueryData(qk.session, null)
      const here = router.state.location.href
      if (!router.state.location.pathname.startsWith('/login')) {
        void router.navigate({ to: '/login', search: { next: here } })
      }
    })
    return () => {
      off()
    }
  }, [queryClient, router])

  return (
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
        <Toaster />
      </QueryClientProvider>
    </ThemeProvider>
  )
}
