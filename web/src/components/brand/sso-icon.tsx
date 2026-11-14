import { KeyRound } from 'lucide-react'
import { isZitadel } from './brands'
import { BrandTile, GLYPH, type TileSize } from './brand-tile'
import { ZitadelMark } from './zitadel-mark'

/**
 * The mark of a single sign-on provider: ZITADEL's when the provider is named
 * Zitadel (any case), a generic key otherwise.
 */
export function SsoProviderIcon({
  name,
  size = 'sm',
  className,
  decorative,
}: {
  name: string | null | undefined
  size?: TileSize
  className?: string
  decorative?: boolean
}) {
  if (isZitadel(name)) {
    return (
      <BrandTile label="ZITADEL" size={size} tone="light" className={className} decorative={decorative}>
        <ZitadelMark className="size-[88%]" />
      </BrandTile>
    )
  }
  return (
    <BrandTile label={name?.trim() || 'Single sign-on'} size={size} tone="neutral" className={className} decorative={decorative}>
      <KeyRound aria-hidden className={GLYPH[size]} />
    </BrandTile>
  )
}
