import { Check, Copy } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/ui/tooltip'
import { useCopy } from '@/hooks/use-copy'
import { cn } from '@/lib/utils'

export function CopyButton({
  value,
  label = 'Copy',
  toastLabel,
  className,
  size = 'icon-xs',
  children,
}: {
  value: string
  label?: string
  toastLabel?: string
  className?: string
  size?: 'icon-xs' | 'icon-sm' | 'xs' | 'sm'
  children?: React.ReactNode
}) {
  const { copied, copy } = useCopy()
  const icon = copied ? <Check className="text-success" /> : <Copy />
  return (
    <Tip content={copied ? 'Copied' : label}>
      <Button
        variant="ghost"
        size={size}
        className={cn('text-muted-foreground hover:text-foreground', className)}
        aria-label={label}
        onClick={(e) => {
          e.stopPropagation()
          void copy(value, toastLabel)
        }}
      >
        {icon}
        {children}
      </Button>
    </Tip>
  )
}

/** Monospace value with a copy button, e.g. a DSN or event id. */
export function CopyField({ value, className, label = 'Copy', mask = false }: { value: string; className?: string; label?: string; mask?: boolean }) {
  return (
    <div className={cn('flex min-w-0 items-center gap-1 rounded-md border bg-code-bg py-0.5 pr-0.5 pl-2.5', className)}>
      <code className="min-w-0 flex-1 truncate font-mono text-[12.5px]" title={mask ? undefined : value}>
        {mask ? '•'.repeat(Math.min(32, value.length)) : value}
      </code>
      <CopyButton value={value} label={label} />
    </div>
  )
}
