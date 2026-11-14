import { cva, type VariantProps } from 'class-variance-authority'
import { Slot } from 'radix-ui'
import * as React from 'react'
import { cn } from '@/lib/utils'

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-md text-sm font-medium whitespace-nowrap transition-[color,background-color,border-color,box-shadow] outline-none select-none focus-visible:ring-[3px] focus-visible:ring-ring/40 focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default: 'bg-primary text-primary-foreground shadow-xs hover:bg-primary/88',
        brand: 'bg-brand text-brand-foreground shadow-xs hover:bg-brand/90',
        destructive: 'bg-destructive text-white shadow-xs hover:bg-destructive/90 focus-visible:ring-destructive/30',
        outline: 'border bg-card shadow-xs hover:bg-accent hover:text-accent-foreground',
        secondary: 'bg-secondary text-secondary-foreground hover:bg-accent',
        ghost: 'hover:bg-accent hover:text-accent-foreground',
        link: 'text-brand-text underline-offset-4 hover:underline',
      },
      size: {
        default: 'h-8 px-3',
        xs: "h-6 gap-1 rounded-sm px-2 text-xs [&_svg:not([class*='size-'])]:size-3.5",
        sm: 'h-7 gap-1 px-2.5 text-[13px]',
        lg: 'h-10 px-5',
        icon: 'size-8',
        'icon-sm': 'size-7',
        'icon-xs': "size-6 rounded-sm [&_svg:not([class*='size-'])]:size-3.5",
      },
    },
    defaultVariants: { variant: 'default', size: 'default' },
  },
)

function Button({
  className,
  variant,
  size,
  asChild = false,
  type,
  ...props
}: React.ComponentProps<'button'> & VariantProps<typeof buttonVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : 'button'
  return <Comp data-slot="button" type={asChild ? undefined : (type ?? 'button')} className={cn(buttonVariants({ variant, size, className }))} {...props} />
}

export { Button, buttonVariants }
