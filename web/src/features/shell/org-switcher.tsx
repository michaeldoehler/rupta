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
    <span className={cn('flex size-6 shrink-0 items-center justify-center rounded-md bg-foreground text-[11px] font-semibold text-background', className)}>
      {name.slice(0, 1).toUpperCase()}
    </span>
  )
}

export function OrgSwitcher({ org, collapsed }: { org: string; collapsed: boolean }) {
  const { data: session } = useSession()
  const navigate = useNavigate()
  const current = session?.orgs.find((o) => o.slug === org)
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" className={cn('h-9 w-full justify-start gap-2 px-1.5', collapsed && 'justify-center px-0')} aria-label="Switch organization">
          <OrgBadge name={current?.name ?? org} />
          {!collapsed && (
            <>
              <span className="min-w-0 flex-1 truncate text-left text-[13px] font-semibold">{current?.name ?? org}</span>
              <ChevronsUpDown className="size-3.5 text-muted-foreground" />
            </>
          )}
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
