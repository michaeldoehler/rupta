import { cn } from '@/lib/utils'

function Kbd({ className, ...props }: React.ComponentProps<'kbd'>) {
  return (
    <kbd
      data-slot="kbd"
      className={cn(
        'pointer-events-none inline-flex h-[18px] min-w-[18px] items-center justify-center gap-0.5 rounded-[4px] border border-b-2 bg-muted px-1 font-sans text-[10.5px] font-medium text-muted-foreground select-none',
        className,
      )}
      {...props}
    />
  )
}

export { Kbd }
