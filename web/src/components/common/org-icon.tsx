import { useState } from 'react'
import { cn } from '@/lib/utils'

/**
 * The icon of an organization: its website's favicon on a light tile (most
 * favicons are drawn for light backgrounds), or its initial when it has
 * none or the image does not load.
 */
export function OrgIcon({ name, iconUrl, className }: { name: string; iconUrl?: string | null; className?: string }) {
  const [broken, setBroken] = useState<string | null>(null)
  const showImage = iconUrl && broken !== iconUrl
  return (
    <span
      aria-hidden
      className={cn(
        'flex size-6 shrink-0 items-center justify-center overflow-hidden rounded-md text-[11px] leading-none font-semibold',
        showImage ? 'bg-foreground p-[3px] ring-1 ring-black/10 ring-inset' : 'bg-palm text-primary-foreground',
        className,
      )}
    >
      {showImage ? (
        <img src={iconUrl} alt="" className="size-full object-contain" onError={() => setBroken(iconUrl)} draggable={false} />
      ) : (
        name.slice(0, 1).toUpperCase()
      )}
    </span>
  )
}
