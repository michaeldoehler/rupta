import { useId } from 'react'
import { cn } from '@/lib/utils'

/** The rupta mark: a tile split by a fracture line. */
export function LogoMark({ className, title = 'rupta' }: { className?: string; title?: string }) {
  const id = useId()
  return (
    <svg viewBox="0 0 24 24" className={cn('size-6 text-brand', className)} role="img" aria-label={title}>
      <defs>
        <mask id={id}>
          <rect width="24" height="24" fill="white" />
          <path d="M15.2 0.5 L10.6 9.4 L14.4 12.6 L8.6 23.5" stroke="black" strokeWidth="2.1" fill="none" strokeLinejoin="miter" />
        </mask>
      </defs>
      <rect x="1.5" y="1.5" width="21" height="21" rx="6" fill="currentColor" mask={`url(#${id})`} />
    </svg>
  )
}

export function Logo({ className, collapsed = false }: { className?: string; collapsed?: boolean }) {
  return (
    <span className={cn('inline-flex items-center gap-2', className)}>
      <LogoMark />
      {!collapsed && <span className="text-[17px] font-semibold tracking-[-0.03em] text-foreground">rupta</span>}
    </span>
  )
}
