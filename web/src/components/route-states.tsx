import { Link, useRouter, type ErrorComponentProps } from '@tanstack/react-router'
import { Compass } from 'lucide-react'
import { ErrorState, EmptyState } from '@/components/common/states'
import { Button } from '@/components/ui/button'

export function RouteError({ error, reset }: ErrorComponentProps) {
  const router = useRouter()
  return (
    <ErrorState
      className="min-h-[50vh]"
      error={error}
      retry={() => {
        reset()
        void router.invalidate()
      }}
    />
  )
}

export function RouteNotFound() {
  return (
    <EmptyState
      className="min-h-[60vh]"
      icon={<Compass />}
      title="Page not found"
      description="The address may be mistyped, or the page was moved or deleted."
      action={
        <Button asChild variant="outline" size="sm">
          <Link to="/">Go to the start page</Link>
        </Button>
      }
    />
  )
}

export function RoutePending() {
  return (
    <div className="flex min-h-[40vh] items-center justify-center" aria-busy="true" aria-label="Loading">
      <div className="h-1 w-24 overflow-hidden rounded-full bg-muted">
        <div className="h-full w-1/3 animate-[loading_1s_ease-in-out_infinite] rounded-full bg-brand" />
      </div>
    </div>
  )
}
