import { Users } from 'lucide-react'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { cn } from '@/lib/utils'

const HUES = [25, 45, 85, 150, 190, 230, 265, 310, 350]

function hue(seed: string) {
  let h = 0
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) >>> 0
  return HUES[h % HUES.length]
}

export function initials(name: string) {
  const parts = name
    .replace(/^#/, '')
    .split(/[\s@._-]+/)
    .filter(Boolean)
  return ((parts[0]?.[0] ?? '?') + (parts[1]?.[0] ?? '')).toUpperCase()
}

export function UserAvatar({ name, src, className, team = false }: { name: string; src?: string | null; className?: string; team?: boolean }) {
  const h = hue(name)
  return (
    <Avatar className={cn('size-6', team && 'rounded-md', className)}>
      {src && <AvatarImage src={src} alt="" />}
      <AvatarFallback
        className={cn('text-[10px] font-semibold', team && 'rounded-md')}
        style={{ background: `oklch(0.9 0.05 ${h})`, color: `oklch(0.38 0.1 ${h})` }}
      >
        {team ? <Users className="size-3" /> : initials(name)}
      </AvatarFallback>
    </Avatar>
  )
}
