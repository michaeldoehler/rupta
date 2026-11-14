import { CalendarClock, Check, ChevronDown, Layers, Server } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useEnvironments, useProjects } from '@/api/queries'
import { PlatformIcon } from '@/components/common/platform-icon'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { PERIOD_LABEL, PERIODS, type Period } from '@/lib/search-params'
import { cn } from '@/lib/utils'
import { useGlobalFilters } from './use-global-filters'

function MultiSelect({
  icon,
  allLabel,
  label,
  options,
  selected,
  onChange,
  searchPlaceholder,
}: {
  icon: React.ReactNode
  allLabel: string
  label: string
  options: { value: string; label: string; icon?: React.ReactNode }[]
  selected: string[]
  onChange: (v: string[]) => void
  searchPlaceholder: string
}) {
  const [open, setOpen] = useState(false)
  const [filter, setFilter] = useState('')
  const shown = options.filter((o) => o.label.toLowerCase().includes(filter.toLowerCase()))
  const current =
    selected.length === 0
      ? allLabel
      : selected.length === 1
        ? (options.find((o) => o.value === selected[0])?.label ?? selected[0])
        : `${selected.length} ${label}`
  const toggle = (v: string) => onChange(selected.includes(v) ? selected.filter((x) => x !== v) : [...selected, v])
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm" className="max-w-52 min-w-0 gap-1.5 font-normal" aria-label={`${label}: ${current}`}>
          {icon}
          <span className="truncate">{current}</span>
          <ChevronDown className="size-3.5 opacity-60" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-64 p-0">
        {options.length > 6 && (
          <div className="border-b p-2">
            <Input autoFocus value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={searchPlaceholder} className="h-7 text-[13px]" />
          </div>
        )}
        <div className="max-h-72 overflow-y-auto p-1" role="listbox" aria-multiselectable="true" aria-label={label}>
          <button
            type="button"
            role="option"
            aria-selected={selected.length === 0}
            className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-[13px] hover:bg-accent"
            onClick={() => onChange([])}
          >
            <span className="flex size-4 items-center justify-center">{selected.length === 0 && <Check className="size-3.5" />}</span>
            {allLabel}
          </button>
          {shown.map((o) => (
            <div
              key={o.value}
              role="option"
              aria-selected={selected.includes(o.value)}
              className="group flex items-center gap-2 rounded-sm px-2 py-1.5 text-[13px] hover:bg-accent"
            >
              <Checkbox checked={selected.includes(o.value)} onCheckedChange={() => toggle(o.value)} aria-label={`Include ${o.label}`} />
              <button type="button" className="flex min-w-0 flex-1 items-center gap-2 text-left" onClick={() => onChange([o.value])} title={`Only ${o.label}`}>
                {o.icon}
                <span className="truncate">{o.label}</span>
              </button>
              <span className="text-[11px] text-muted-foreground opacity-0 group-hover:opacity-100">only</span>
            </div>
          ))}
          {shown.length === 0 && <p className="px-2 py-3 text-center text-xs text-muted-foreground">No matches</p>}
        </div>
      </PopoverContent>
    </Popover>
  )
}

export function ProjectSelector() {
  const { org, projects, update } = useGlobalFilters()
  const { data } = useProjects(org)
  const options = useMemo(() => (data ?? []).map((p) => ({ value: p.id, label: p.slug, icon: <PlatformIcon platform={p.platform} size="sm" /> })), [data])
  return (
    <MultiSelect
      icon={<Layers className="size-3.5 opacity-70" />}
      allLabel="All projects"
      label="projects"
      options={options}
      selected={projects}
      onChange={(v) => update({ project: v })}
      searchPlaceholder="Filter projects"
    />
  )
}

export function EnvironmentSelector() {
  const { org, projects, environments, update } = useGlobalFilters()
  const { data } = useEnvironments(org, projects)
  const options = useMemo(() => (data ?? []).map((e) => ({ value: e, label: e })), [data])
  return (
    <MultiSelect
      icon={<Server className="size-3.5 opacity-70" />}
      allLabel="All environments"
      label="environments"
      options={options}
      selected={environments}
      onChange={(v) => update({ environment: v })}
      searchPlaceholder="Filter environments"
    />
  )
}

export function PeriodSelector({ className }: { className?: string }) {
  const { period, update } = useGlobalFilters()
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" className={cn('gap-1.5 font-normal', className)}>
          <CalendarClock className="size-3.5 opacity-70" />
          <span className="sr-only">Period:</span>
          <span className="hidden sm:inline">{PERIOD_LABEL[period]}</span>
          <span className="sm:hidden">{period}</span>
          <ChevronDown className="size-3.5 opacity-60" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        <DropdownMenuRadioGroup value={period} onValueChange={(v) => update({ period: v as Period })}>
          {PERIODS.map((p) => (
            <DropdownMenuRadioItem key={p} value={p}>
              {PERIOD_LABEL[p]}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
