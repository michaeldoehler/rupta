import { useNavigate } from '@tanstack/react-router'
import { Check, ChevronsUpDown, Plus } from 'lucide-react'
import { useSession } from '@/api/queries'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'

function OrgBadge({ name, className }: { name: string; className?: string }) {
  return (
    <span className={cn('flex size-6 shrink-0 items-center justify-center rounded-md bg-palm text-[11px] font-semibold text-primary-foreground', className)}>
      {name.slice(0, 1).toUpperCase()}
    </span>
  )
}

/** Organization menu; `compact` for the top bar. */
export function OrgSwitcher({ org, compact = false }: { org: string; compact?: boolean }) {
  const { data: session } = useSession()
  const navigate = useNavigate()
  const current = session?.orgs.find((o) => o.slug === org)
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          className={cn('justify-start gap-2 px-1.5', compact ? 'h-8 max-w-48' : 'h-9 w-full')}
          aria-label={`Organization: ${current?.name ?? org}`}
        >
          <OrgBadge name={current?.name ?? org} className={compact ? 'size-5 text-[10px]' : undefined} />
          <span className="min-w-0 flex-1 truncate text-left text-[13px] font-semibold">{current?.name ?? org}</span>
          <ChevronsUpDown className="size-3.5 text-muted-foreground" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-60">
        <DropdownMenuLabel>Organizations</DropdownMenuLabel>
        {session?.orgs.map((o) => (
          <DropdownMenuItem key={o.id} onSelect={() => navigate({ to: '/$org/issues', params: { org: o.slug } })}>
            <OrgBadge name={o.name} className="size-5 text-[10px]" />
            <span className="flex-1 truncate">{o.name}</span>
            <span className="text-[11px] text-muted-foreground capitalize">{o.role}</span>
            {o.slug === org && <Check className="size-3.5" />}
          </DropdownMenuItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => navigate({ to: '/onboarding' })}>
          <Plus /> New organization
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
