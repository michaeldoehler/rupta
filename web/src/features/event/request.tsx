import type { EventRequest } from '@/api/types'
import { CopyButton } from '@/components/common/copy-button'
import { KeyValueTable } from '@/components/common/page'
import { Badge } from '@/components/ui/badge'
import { ValueView } from './value-view'

function pairs(v: EventRequest['query_string'] | EventRequest['cookies']): [string, string][] {
  if (!v) return []
  if (typeof v === 'string')
    return v
      .split(/[&;]\s*/)
      .filter(Boolean)
      .map((kv) => {
        const [k, ...rest] = kv.split('=')
        return [decodeURIComponent(k), decodeURIComponent(rest.join('='))]
      })
  if (Array.isArray(v)) return v
  return Object.entries(v)
}

/** A cURL command that reproduces the request (filtered values stay filtered). */
export function toCurl(req: EventRequest): string {
  const qs = pairs(req.query_string)
  const url = req.url
    ? `${req.url}${qs.length && !req.url.includes('?') ? `?${qs.map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join('&')}` : ''}`
    : ''
  const parts = [`curl -X ${req.method ?? 'GET'} '${url.replace(/'/g, "'\\''")}'`]
  for (const [k, v] of req.headers ?? []) parts.push(`  -H '${k}: ${String(v).replace(/'/g, "'\\''")}'`)
  if (req.data !== undefined && req.data !== null) {
    const body = typeof req.data === 'string' ? req.data : JSON.stringify(req.data)
    parts.push(`  --data '${body.replace(/'/g, "'\\''")}'`)
  }
  return parts.join(' \\\n')
}

export function RequestSection({ req }: { req: EventRequest }) {
  const qs = pairs(req.query_string)
  const cookies = pairs(req.cookies)
  return (
    <div className="space-y-3">
      <div className="flex min-w-0 items-center gap-2 rounded-lg border bg-card px-3 py-2">
        <Badge variant="outline" className="font-mono font-semibold text-foreground">
          {req.method ?? 'GET'}
        </Badge>
        <a href={req.url ?? undefined} target="_blank" rel="noreferrer noopener" className="min-w-0 flex-1 truncate font-mono text-[13px] hover:underline">
          {req.url}
        </a>
        <CopyButton value={toCurl(req)} label="Copy as cURL" toastLabel="cURL command copied" size="xs">
          cURL
        </CopyButton>
      </div>
      {qs.length > 0 && (
        <Sub title="Query string">
          <KeyValueTable rows={qs} />
        </Sub>
      )}
      {req.data !== undefined && req.data !== null && (
        <Sub title="Body">
          <pre className="overflow-x-auto rounded-lg border bg-code-bg p-3 font-mono text-[12.5px] leading-relaxed">
            {typeof req.data === 'string' ? tryPretty(req.data) : JSON.stringify(req.data, null, 2)}
          </pre>
        </Sub>
      )}
      {req.headers && req.headers.length > 0 && (
        <Sub title="Headers">
          <KeyValueTable rows={req.headers.map(([k, v]) => [k, <ValueView key={k} value={v} />])} />
        </Sub>
      )}
      {cookies.length > 0 && (
        <Sub title="Cookies">
          <KeyValueTable rows={cookies} />
        </Sub>
      )}
      {req.env && Object.keys(req.env).length > 0 && (
        <Sub title="Environment">
          <KeyValueTable rows={Object.entries(req.env)} />
        </Sub>
      )}
    </div>
  )
}

function tryPretty(s: string) {
  try {
    return JSON.stringify(JSON.parse(s), null, 2)
  } catch {
    return s
  }
}

function Sub({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h4 className="mb-1.5 text-xs font-medium text-muted-foreground">{title}</h4>
      {children}
    </div>
  )
}
