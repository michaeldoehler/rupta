import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { Bar, BarChart, ResponsiveContainer, Tooltip, XAxis } from 'recharts'
import { api } from '@/api/endpoints'
import { qk } from '@/api/queries'
import { Skeleton } from '@/components/ui/skeleton'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { absolute, thousands, plural } from '@/lib/format'

export function IssueChart({ issueId }: { issueId: string }) {
  const [period, setPeriod] = useState<'24h' | '30d'>('24h')
  const q = useQuery({ queryKey: qk.issues.stats(issueId, period), queryFn: () => api.issues.stats(issueId, period), staleTime: 60_000 })
  const data = (q.data ?? []).map(([ts, count]) => ({ ts: ts * 1000, count }))
  const total = data.reduce((s, d) => s + d.count, 0)
  return (
    <div className="min-w-0">
      <div className="mb-1 flex items-center justify-between gap-2">
        <span className="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
          {thousands(total)} {plural(total, 'event')} · last {period === '24h' ? '24 hours' : '30 days'}
        </span>
        <ToggleGroup type="single" value={period} onValueChange={(v) => v && setPeriod(v as '24h' | '30d')} aria-label="Chart period" className="h-6">
          <ToggleGroupItem value="24h" className="px-1.5 text-[11px]">
            24h
          </ToggleGroupItem>
          <ToggleGroupItem value="30d" className="px-1.5 text-[11px]">
            30d
          </ToggleGroupItem>
        </ToggleGroup>
      </div>
      <div className="h-16" role="img" aria-label={`${total} events in the last ${period}`}>
        {q.isPending ? (
          <Skeleton className="h-full w-full" />
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={data} margin={{ top: 2, right: 0, bottom: 0, left: 0 }} barCategoryGap={1}>
              <XAxis dataKey="ts" hide />
              <Tooltip
                cursor={{ fill: 'var(--accent)' }}
                isAnimationActive={false}
                content={({ active, payload }) =>
                  active && payload?.length ? (
                    <div className="rounded-md border bg-popover px-2 py-1 text-xs shadow-md">
                      <div className="font-medium tabular">{thousands(payload[0].value as number)} events</div>
                      <div className="text-muted-foreground">{absolute(payload[0].payload.ts as number)}</div>
                    </div>
                  ) : null
                }
              />
              <Bar dataKey="count" fill="var(--brand)" radius={[2, 2, 0, 0]} isAnimationActive={false} />
            </BarChart>
          </ResponsiveContainer>
        )}
      </div>
    </div>
  )
}
