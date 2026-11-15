import { BrandTile, GLYPH, type TileSize } from '@/components/brand/brand-tile'
import type { Mark } from '@/components/brand/marks'
import * as m from '@/components/brand/platform-marks'

/**
 * How a platform's mark is drawn: `light` puts the brand-coloured glyph on a
 * light tile, like the service marks; `dark` on a dark tile, only for
 * JavaScript, whose mark is a yellow square that needs a dark surround.
 */
const PLATFORMS: Record<string, { mark: Mark; tile: 'light' | 'dark' }> = {
  javascript: { mark: m.javascript, tile: 'dark' },
  // React's own docs draw the mark in #087EA4 on light backgrounds.
  'javascript-react': { mark: { ...m.react, hex: '#087EA4' }, tile: 'light' },
  'javascript-nextjs': { mark: m.nextjs, tile: 'light' },
  'javascript-vue': { mark: m.vue, tile: 'light' },
  node: { mark: m.nodejs, tile: 'light' },
  python: { mark: m.python, tile: 'light' },
  'python-django': { mark: m.django, tile: 'light' },
  'python-flask': { mark: m.flask, tile: 'light' },
  'python-fastapi': { mark: m.fastapi, tile: 'light' },
  php: { mark: m.php, tile: 'light' },
  'php-laravel': { mark: m.laravel, tile: 'light' },
  'php-symfony': { mark: m.symfony, tile: 'light' },
  go: { mark: m.go, tile: 'light' },
  java: { mark: m.openjdk, tile: 'light' },
  dotnet: { mark: m.dotnet, tile: 'light' },
  ruby: { mark: m.ruby, tile: 'light' },
  rust: { mark: m.rust, tile: 'light' },
}

const NAMES: Record<string, string> = {
  javascript: 'JavaScript',
  'javascript-react': 'React',
  'javascript-nextjs': 'Next.js',
  'javascript-vue': 'Vue',
  node: 'Node.js',
  python: 'Python',
  'python-django': 'Django',
  'python-flask': 'Flask',
  'python-fastapi': 'FastAPI',
  php: 'PHP',
  'php-laravel': 'Laravel',
  'php-symfony': 'Symfony',
  go: 'Go',
  java: 'Java',
  dotnet: '.NET',
  ruby: 'Ruby',
  rust: 'Rust',
}

export function platformLabel(platform: string | null | undefined): string {
  if (!platform) return 'Other'
  return NAMES[platform] ?? NAMES[platform.split('-')[0]] ?? platform
}

/** The platform of a project or event, e.g. `javascript-react` or `python`. */
export function platformOf(platform: string | null | undefined) {
  return (platform && (PLATFORMS[platform] ?? PLATFORMS[platform.split('-')[0]])) || null
}

const SIZE: Record<'sm' | 'md' | 'lg', TileSize> = { sm: 'xs', md: 'sm', lg: 'lg' }

/**
 * The platform's mark on a tile. Platforms without a mark (and projects
 * without a platform) get a neutral tile with the first letter. Decorative:
 * it always sits next to a name, and the title names the platform on hover.
 */
export function PlatformIcon({ platform, className, size = 'md' }: { platform: string | null | undefined; className?: string; size?: 'sm' | 'md' | 'lg' }) {
  const p = platformOf(platform)
  const tile = SIZE[size]
  const label = platformLabel(platform)
  if (!p) {
    return (
      <BrandTile label={label} size={tile} tone="neutral" decorative className={className}>
        <span aria-hidden className="text-muted-foreground">
          {(platform ?? '·').charAt(0).toUpperCase()}
        </span>
      </BrandTile>
    )
  }
  return (
    <BrandTile label={label} size={tile} tone={p.tile} decorative className={className}>
      <svg viewBox="0 0 24 24" aria-hidden className={p.tile === 'dark' ? 'size-full' : GLYPH[tile]} data-mark={p.mark.title} fill={p.mark.hex}>
        <path d={p.mark.path} />
      </svg>
    </BrandTile>
  )
}
