import { cn } from '@/lib/utils'

// Neutral, text-based platform badges (no third-party logos).
const PLATFORMS: Record<string, { label: string; hue: number }> = {
  javascript: { label: 'JS', hue: 95 },
  'javascript-react': { label: 'Re', hue: 220 },
  'javascript-nextjs': { label: 'Nx', hue: 270 },
  'javascript-vue': { label: 'Vu', hue: 155 },
  node: { label: 'Nd', hue: 145 },
  python: { label: 'Py', hue: 250 },
  'python-django': { label: 'Dj', hue: 165 },
  'python-flask': { label: 'Fl', hue: 250 },
  'python-fastapi': { label: 'FA', hue: 180 },
  php: { label: 'php', hue: 280 },
  'php-laravel': { label: 'Lv', hue: 25 },
  'php-symfony': { label: 'Sf', hue: 0 },
  go: { label: 'Go', hue: 205 },
  java: { label: 'Jv', hue: 40 },
  dotnet: { label: '.N', hue: 290 },
  ruby: { label: 'Rb', hue: 15 },
  rust: { label: 'Rs', hue: 50 },
}

export function platformLabel(platform: string | null | undefined): string {
  if (!platform) return 'Other'
  const names: Record<string, string> = {
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
  return names[platform] ?? platform
}

export function PlatformIcon({ platform, className, size = 'md' }: { platform: string | null | undefined; className?: string; size?: 'sm' | 'md' | 'lg' }) {
  const p = (platform && PLATFORMS[platform]) || (platform && PLATFORMS[platform.split('-')[0]]) || { label: '?', hue: 60 }
  return (
    <span
      title={platformLabel(platform)}
      aria-hidden
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-[5px] font-mono leading-none font-semibold tracking-tight',
        'bg-[oklch(0.93_0.045_var(--h))] text-[oklch(0.4_0.11_var(--h))] dark:bg-[oklch(0.3_0.06_var(--h))] dark:text-[oklch(0.86_0.09_var(--h))]',
        size === 'sm' && 'size-4 text-[7.5px]',
        size === 'md' && 'size-5 text-[9px]',
        size === 'lg' && 'size-8 rounded-md text-[12px]',
        className,
      )}
      style={{ '--h': p.hue } as React.CSSProperties}
    >
      {p.label}
    </span>
  )
}
