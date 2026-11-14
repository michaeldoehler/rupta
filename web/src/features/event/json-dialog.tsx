import { Download } from 'lucide-react'
import { useMemo } from 'react'
import type { RuptaEvent } from '@/api/types'
import { CopyButton } from '@/components/common/copy-button'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'

export function downloadJson(event: RuptaEvent) {
  const blob = new Blob([JSON.stringify(event, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `event-${event.event_id}.json`
  a.click()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}

export function JsonDialog({ event, open, onOpenChange }: { event: RuptaEvent; open: boolean; onOpenChange: (o: boolean) => void }) {
  const json = useMemo(() => (open ? JSON.stringify(event, null, 2) : ''), [event, open])
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[85vh] flex-col sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>Event JSON</DialogTitle>
          <DialogDescription className="font-mono text-xs">{event.event_id}</DialogDescription>
        </DialogHeader>
        <div className="flex gap-2">
          <CopyButton value={json} label="Copy JSON" toastLabel="JSON copied" size="sm">
            Copy
          </CopyButton>
          <Button variant="ghost" size="sm" onClick={() => downloadJson(event)}>
            <Download /> Download
          </Button>
        </div>
        <pre className="min-h-0 flex-1 overflow-auto rounded-md border bg-code-bg p-3 font-mono text-[12px] leading-relaxed">{json}</pre>
      </DialogContent>
    </Dialog>
  )
}
