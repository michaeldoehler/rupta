import { createFileRoute, redirect } from '@tanstack/react-router'

export const Route = createFileRoute('/_auth/$org/')({
  beforeLoad: ({ params }) => {
    throw redirect({ to: '/$org/issues', params })
  },
})
