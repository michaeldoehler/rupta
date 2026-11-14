import type { ReactNode } from 'react'
import { Logo, LogoMark } from '@/components/common/logo'

/**
 * Sign-in, setup and invitations. The one deliberate brand moment: a panel
 * with the palette gradient (olive charcoal → palm leaf → electric lime).
 */
export function AuthLayout({ title, subtitle, children, footer }: { title: string; subtitle?: ReactNode; children: ReactNode; footer?: ReactNode }) {
  return (
    <main className="relative grid min-h-svh lg:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
      <div aria-hidden className="absolute inset-x-0 top-0 h-1 [background:linear-gradient(90deg,#404436,#779148,#caf534)] lg:hidden" />
      <aside className="relative hidden overflow-hidden lg:block" aria-hidden>
        <div className="absolute inset-0 [background:var(--brand-gradient)]" />
        <div className="absolute inset-0 [background-image:radial-gradient(rgb(0_0_0/0.10)_1px,transparent_1px)] [background-size:18px_18px]" />
        <div className="relative flex h-full flex-col justify-between p-10">
          <span className="inline-flex items-center gap-2 text-[#eef2e3]">
            <LogoMark className="size-7 text-[#caf534]" />
            <span className="text-xl font-semibold tracking-[-0.03em]">rupta</span>
          </span>
          <div className="max-w-md">
            <p className="text-4xl leading-[1.05] font-semibold tracking-[-0.04em] text-[#171a0d]">See what breaks, and why.</p>
            <p className="mt-3 text-[15px] text-[#20240f]">
              Errors, traces, logs, metrics, profiles and session replays from the official open-source SDKs, on your own servers.
            </p>
          </div>
        </div>
      </aside>
      <div className="relative flex flex-col items-center justify-center px-4 py-10">
        <div className="w-full max-w-[400px]">
          <div className="mb-6 flex justify-center lg:hidden">
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
      </div>
    </main>
  )
}
