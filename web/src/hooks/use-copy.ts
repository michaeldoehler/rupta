import * as React from 'react'
import { toast } from 'sonner'

export async function copyText(text: string, label = 'Copied to clipboard') {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    const ta = document.createElement('textarea')
    ta.value = text
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    document.execCommand('copy')
    ta.remove()
  }
  toast.success(label)
}

export function useCopy(timeout = 1500) {
  const [copied, setCopied] = React.useState(false)
  const copy = React.useCallback(
    async (text: string, label?: string) => {
      await copyText(text, label)
      setCopied(true)
      setTimeout(() => setCopied(false), timeout)
    },
    [timeout],
  )
  return { copied, copy }
}
