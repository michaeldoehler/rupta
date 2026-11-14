import type { ReactNode } from 'react'
import { Logo } from '@/components/common/logo'

export function AuthLayout({ title, subtitle, children, footer }: { title: string; subtitle?: ReactNode; children: ReactNode; footer?: ReactNode }) {
  return (
    <main className="relative flex min-h-svh flex-col items-center justify-center overflow-hidden px-4 py-10">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 [background-image:radial-gradient(var(--border)_1px,transparent_1px)] [mask-image:radial-gradient(ellipse_at_center,black_20%,transparent_70%)] [background-size:22px_22px]"
      />
      <div className="relative w-full max-w-[400px]">
        <div className="mb-6 flex justify-center">
          <Logo />
        </div>
        <div className="rounded-xl border bg-card p-6 shadow-sm sm:p-7">
          <div className="mb-5 space-y-1">
            <h1 className="text-lg font-semibold tracking-tight">{title}</h1>
            {subtitle && <p className="text-[13px] text-muted-foreground">{subtitle}</p>}
          </div>
          {children}
        </div>
        {footer && <div className="mt-4 text-center text-xs text-muted-foreground">{footer}</div>}
      </div>
    </main>
  )
}
