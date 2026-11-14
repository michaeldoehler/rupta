import { brandOf } from './brands'
import { BrandTile, GLYPH, type TileSize } from './brand-tile'

/**
 * The mark of a service (issue tracker or notification channel) on a small
 * tile, named for assistive technology. Unknown kinds get a generic icon.
 * `label` overrides the accessible name (for example the integration's name).
 */
export function BrandIcon({
  kind,
  size = 'md',
  label,
  className,
  decorative,
}: {
  kind: string | null | undefined
  size?: TileSize
  label?: string
  className?: string
  decorative?: boolean
}) {
  const b = brandOf(kind)
  const name = label ?? b.label
  if (b.tile === 'neutral') {
    const Icon = b.icon
    return (
      <BrandTile label={name} size={size} tone="neutral" className={className} decorative={decorative}>
        <Icon aria-hidden strokeWidth={2} className={GLYPH[size]} />
      </BrandTile>
    )
  }
  if (b.tile === 'monogram') {
    return (
      <BrandTile label={name} size={size} tone="solid" background={b.color} className={className} decorative={decorative}>
        <span aria-hidden data-monogram>
          {b.letter}
        </span>
      </BrandTile>
    )
  }
  return (
    <BrandTile label={name} size={size} tone={b.tile} background={b.tile === 'solid' ? b.mark.hex : undefined} className={className} decorative={decorative}>
      <svg viewBox="0 0 24 24" aria-hidden className={GLYPH[size]} data-mark={b.mark.title} fill={b.tile === 'light' ? b.mark.hex : 'currentColor'}>
        <path d={b.mark.path} />
      </svg>
    </BrandTile>
  )
}
