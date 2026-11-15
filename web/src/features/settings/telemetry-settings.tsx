import { useEffect, useState } from 'react'
import type { TelemetrySettings } from '@/api/telemetry-types'
import { CardsSkeleton } from '@/components/common/states'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { useRole } from './org-pages'
import { useProject, useSaveSettings } from './project-pages'
import { SettingsCard, SettingsPage } from './settings-layout'

// Defaults of the server (rupta-core settings.rs).
export const TELEMETRY_DEFAULTS: TelemetrySettings = {
  retention: { logs: 30, spans: 30, metrics: 90, profiles: 30, replays: 30 },
  sampling: { transactions: 1, spans: 1, logs: 1 },
  data_types: { traces: true, logs: true, metrics: true, profiles: true, replays: true },
}

type Kind = keyof TelemetrySettings['data_types']

const KINDS: {
  kind: Kind
  title: string
  description: string
  retention: keyof TelemetrySettings['retention']
  sampling?: (keyof TelemetrySettings['sampling'])[]
}[] = [
  {
    kind: 'traces',
    title: 'Traces',
    description: 'Transactions and spans, the data of performance and the trace view.',
    retention: 'spans',
    sampling: ['transactions', 'spans'],
  },
  { kind: 'logs', title: 'Logs', description: 'Log records from the SDKs’ logger APIs and OpenTelemetry logs.', retention: 'logs', sampling: ['logs'] },
  { kind: 'metrics', title: 'Metrics', description: 'Counters, gauges, distributions and sets.', retention: 'metrics' },
  { kind: 'profiles', title: 'Profiles', description: 'Sampled transaction profiles and continuous profiling chunks.', retention: 'profiles' },
  {
    kind: 'replays',
    title: 'Session replays',
    description: 'Recordings of browser sessions (mobile video replays are accepted and dropped).',
    retention: 'replays',
  },
]

const SAMPLING_LABEL: Record<keyof TelemetrySettings['sampling'], string> = {
  transactions: 'Transactions (and their spans)',
  spans: 'Standalone spans',
  logs: 'Log records',
}

const DAYS = [7, 14, 30, 60, 90, 180, 365]

/** Merges stored settings over the server defaults. */
export function telemetryFrom(settings: Record<string, unknown> | undefined): TelemetrySettings {
  const s = (settings ?? {}) as Partial<TelemetrySettings>
  return {
    retention: { ...TELEMETRY_DEFAULTS.retention, ...s.retention },
    sampling: { ...TELEMETRY_DEFAULTS.sampling, ...s.sampling },
    data_types: { ...TELEMETRY_DEFAULTS.data_types, ...s.data_types },
  }
}

function SamplingInput({ value, onChange, disabled, label }: { value: number; onChange: (v: number) => void; disabled: boolean; label: string }) {
  const [text, setText] = useState(String(Math.round(value * 1000) / 10))
  useEffect(() => setText(String(Math.round(value * 1000) / 10)), [value])
  const n = Number(text)
  const invalid = text.trim() === '' || !Number.isFinite(n) || n < 0 || n > 100
  return (
    <div className="flex items-center gap-2">
      <input
        type="range"
        min={0}
        max={100}
        step={1}
        value={invalid ? value * 100 : n}
        disabled={disabled}
        onChange={(e) => {
          setText(e.target.value)
          onChange(Number(e.target.value) / 100)
        }}
        className="w-40 accent-[var(--brand)]"
        aria-label={`${label}: share kept`}
      />
      <Input
        value={text}
        disabled={disabled}
        inputMode="decimal"
        aria-label={`${label}: percent kept`}
        aria-invalid={invalid || undefined}
        onChange={(e) => {
          setText(e.target.value)
          const v = Number(e.target.value)
          if (Number.isFinite(v) && v >= 0 && v <= 100) onChange(v / 100)
        }}
        className="h-7 w-20 text-right tabular"
      />
      <span className="text-xs text-muted-foreground">%</span>
    </div>
  )
}

export function ProjectTelemetryPage({ org, project }: { org: string; project: string }) {
  const q = useProject(org, project)
  const save = useSaveSettings(org, project)
  const { can } = useRole('admin')
  const [draft, setDraft] = useState<TelemetrySettings>(TELEMETRY_DEFAULTS)
  useEffect(() => {
    if (q.data) setDraft(telemetryFrom(q.data.settings))
  }, [q.data])
  if (!q.data)
    return (
      <SettingsPage title="Telemetry data">
        <CardsSkeleton count={3} />
      </SettingsPage>
    )
  const stored = telemetryFrom(q.data.settings)
  const dirty = JSON.stringify(stored) !== JSON.stringify(draft)
  return (
    <SettingsPage
      title="Telemetry data"
      description="Turn data types on or off, choose how long they are kept and what share the server keeps at ingestion. Error events have their own retention under Rate limits & retention."
      actions={
        <div className="flex gap-2">
          <Button size="sm" variant="outline" disabled={!dirty || save.isPending} onClick={() => setDraft(stored)}>
            Discard
          </Button>
          <Button
            size="sm"
            disabled={!can || !dirty || save.isPending}
            onClick={() => save.mutate({ retention: draft.retention, sampling: draft.sampling, data_types: draft.data_types })}
          >
            Save changes
          </Button>
        </div>
      }
    >
      {KINDS.map((k) => {
        const on = draft.data_types[k.kind]
        return (
          <SettingsCard key={k.kind} title={k.title} description={k.description}>
            <div className="space-y-4">
              <div className="flex items-center justify-between gap-4">
                <div>
                  <div className="text-[13px] font-medium">Accept {k.title.toLowerCase()}</div>
                  <div className="text-xs text-muted-foreground">When off, SDKs still get 200 and the data is counted as filtered, not stored.</div>
                </div>
                <Switch
                  checked={on}
                  disabled={!can}
                  onCheckedChange={(v) => setDraft((d) => ({ ...d, data_types: { ...d.data_types, [k.kind]: v } }))}
                  aria-label={`Accept ${k.title.toLowerCase()}`}
                />
              </div>
              <div className="flex flex-wrap items-center justify-between gap-4">
                <div className="text-[13px] font-medium">Keep for</div>
                <Select
                  value={String(draft.retention[k.retention])}
                  disabled={!can}
                  onValueChange={(v) => setDraft((d) => ({ ...d, retention: { ...d.retention, [k.retention]: Number(v) } }))}
                >
                  <SelectTrigger className="w-40" aria-label={`${k.title} retention`}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {[...new Set([...DAYS, draft.retention[k.retention]])]
                      .sort((a, b) => a - b)
                      .map((d) => (
                        <SelectItem key={d} value={String(d)}>
                          {d} days{d === TELEMETRY_DEFAULTS.retention[k.retention] ? ' (default)' : ''}
                        </SelectItem>
                      ))}
                  </SelectContent>
                </Select>
              </div>
              {k.sampling?.map((sk) => (
                <div key={sk} className="flex flex-wrap items-center justify-between gap-4">
                  <div>
                    <div className="text-[13px] font-medium">Sample {SAMPLING_LABEL[sk].toLowerCase()}</div>
                    <div className="text-xs text-muted-foreground">
                      {sk === 'logs'
                        ? 'Logs with a trace id follow their trace; others are kept at random.'
                        : sk === 'transactions'
                          ? 'By trace id, so a trace is kept or dropped as a whole. Applies after the SDK’s own sample rate.'
                          : 'Spans sent on their own, outside a transaction, by trace id like transactions, so they stay with their trace.'}
                    </div>
                  </div>
                  <SamplingInput
                    label={SAMPLING_LABEL[sk]}
                    value={draft.sampling[sk]}
                    disabled={!can || !on}
                    onChange={(v) => setDraft((d) => ({ ...d, sampling: { ...d.sampling, [sk]: v } }))}
                  />
                </div>
              ))}
            </div>
          </SettingsCard>
        )
      })}
    </SettingsPage>
  )
}
