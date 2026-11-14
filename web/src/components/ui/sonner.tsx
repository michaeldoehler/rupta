import { Toaster as Sonner, type ToasterProps } from 'sonner'
import { useTheme } from '@/components/theme-provider'

function Toaster(props: ToasterProps) {
  const { resolved } = useTheme()
  return (
    <Sonner
      theme={resolved}
      className="toaster group"
      position="bottom-right"
      toastOptions={{
        classNames: {
          toast: 'group toast !bg-popover !text-popover-foreground !border-border !shadow-lg !rounded-lg !text-[13px]',
          description: '!text-muted-foreground',
          actionButton: '!bg-primary !text-primary-foreground',
        },
      }}
      {...props}
    />
  )
}

export { Toaster }
