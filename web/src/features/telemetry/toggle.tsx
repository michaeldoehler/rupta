import type { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

/** A button that stays pressed (aria-pressed), styled like the toolbar buttons. */
export function Toggle({ pressed, onPressedChange, className, ...props }: Omit<ComponentProps<typeof Button>, 'onClick'> & { pressed: boolean; onPressedChange: (v: boolean) => void }) {
  return (
    <Button
      variant="outline"
      size="sm"
      aria-pressed={pressed}
      onClick={() => onPressedChange(!pressed)}
      className={cn('font-normal', pressed && 'border-brand/50 bg-brand-soft text-brand-text hover:bg-brand-soft', className)}
      {...props}
    />
  )
}
