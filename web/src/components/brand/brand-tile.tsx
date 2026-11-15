import { cn } from '@/lib/utils'

export type TileSize = 'xs' | 'sm' | 'md' | 'lg' | 'xl'

/** Tiles of 16, 20, 24, 32 and 40 px. */
const TILE: Record<TileSize, string> = {
  xs: 'size-4 rounded-[4px] text-[9px]',
  sm: 'size-5 rounded-[5px] text-[10px]',
  md: 'size-6 rounded-md text-[11px]',
  lg: 'size-8 rounded-md text-[13px]',
  xl: 'size-10 rounded-lg text-[15px]',
}

/**
 * The glyph inside a tile, set on the glyph itself so surrounding rules for
 * icons (menus, buttons) do not resize it.
 */
export const GLYPH: Record<TileSize, string> = {
  xs: 'size-3',
  sm: 'size-3.5',
  md: 'size-3.5',
  lg: 'size-[18px]',
  xl: 'size-[22px]',
}

/**
 * The rounded tile every service mark sits on, so marks of different shapes
 * line up. `tone` picks the surface: light (for dark or brand-coloured
 * glyphs), dark (for light brand colours), solid (the brand colour, given in `background`) or neutral.
 */
export function BrandTile({
  label,
  size = 'md',
  tone,
  background,
  decorative,
  className,
  children,
}: {
  label: string
  /** Next to text that already names the service: hidden from assistive technology. */
  decorative?: boolean
  size?: TileSize
  tone: 'light' | 'dark' | 'solid' | 'neutral'
  background?: string
  className?: string
  children: React.ReactNode
}) {
  return (
    <span
      role={decorative ? undefined : 'img'}
      aria-label={decorative ? undefined : label}
      aria-hidden={decorative || undefined}
      title={label}
      data-brand-tile={tone}
      style={background ? { backgroundColor: background } : undefined}
      className={cn(
        'inline-flex shrink-0 items-center justify-center leading-none font-semibold select-none',
        // A light tile keeps the brand colours of the glyph; a thin edge
        // separates the dark brand colours from the olive surfaces.
        tone === 'light' && 'bg-foreground ring-1 ring-black/10 ring-inset',
        tone === 'dark' && 'bg-[#1b1b18] ring-1 ring-white/10 ring-inset',
        tone === 'solid' && 'text-white ring-1 ring-white/15 ring-inset',
        tone === 'neutral' && 'border border-border-strong bg-secondary text-foreground',
        TILE[size],
        className,
      )}
    >
      {children}
    </span>
  )
}
