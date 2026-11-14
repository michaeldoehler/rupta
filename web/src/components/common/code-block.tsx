import { useMemo } from 'react'
import { useHighlight, type Lang } from '@/features/event/highlight'
import { cn } from '@/lib/utils'
import { CopyButton } from './copy-button'

/** Highlighted, copyable code snippet (highlighting loads lazily). */
export function CodeBlock({ code, lang, className, title }: { code: string; lang?: Lang; className?: string; title?: string }) {
  const lines = useMemo(() => code.split('\n'), [code])
  const tokens = useHighlight(lines, lang ?? null)
  return (
    <div className={cn('group relative overflow-hidden rounded-lg border bg-code-bg', className)}>
      {title && <div className="border-b px-3 py-1.5 font-mono text-[11.5px] text-muted-foreground">{title}</div>}
      <div className="absolute top-1.5 right-1.5 opacity-70 group-hover:opacity-100">
        <CopyButton value={code} label="Copy code" toastLabel="Code copied" />
      </div>
      <pre className="shiki-code overflow-x-auto p-3 pr-10 font-mono text-[12.5px] leading-relaxed">
        <code>
          {lines.map((line, i) => (
            <span key={i} className="block">
              {tokens?.[i]
                ? tokens[i].map((t, j) => (
                    <span key={j} style={{ '--shiki-light': t.light, '--shiki-dark': t.dark } as React.CSSProperties}>
                      {t.content}
                    </span>
                  ))
                : line || ' '}
            </span>
          ))}
        </code>
      </pre>
    </div>
  )
}
