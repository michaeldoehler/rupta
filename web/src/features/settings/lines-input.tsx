import { Textarea } from '@/components/ui/textarea'

/** Edits a list of strings, one per line. */
export function LinesInput({
  value,
  onChange,
  placeholder,
  rows = 4,
  id,
  ...rest
}: {
  value: string[]
  onChange: (v: string[]) => void
  placeholder?: string
  rows?: number
  id?: string
  'aria-describedby'?: string
  'aria-invalid'?: boolean
}) {
  return (
    <Textarea
      id={id}
      {...rest}
      rows={rows}
      className="font-mono text-[12.5px]"
      placeholder={placeholder}
      value={value.join('\n')}
      onChange={(e) => onChange(e.target.value.split('\n'))}
      onBlur={(e) =>
        onChange(
          e.target.value
            .split('\n')
            .map((s) => s.trim())
            .filter(Boolean),
        )
      }
    />
  )
}
