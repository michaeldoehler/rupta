import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Kbd } from '@/components/ui/kbd'

export const SHORTCUTS: { group: string; items: [string, string][] }[] = [
  {
    group: 'Global',
    items: [
      ['⌘ K', 'Command palette'],
      ['/', 'Focus search'],
      ['?', 'Show this help'],
      ['g i', 'Go to issues'],
      ['g r', 'Go to releases'],
      ['g c', 'Go to crons'],
      ['g f', 'Go to user feedback'],
      ['g a', 'Go to alerts'],
      ['g s', 'Go to settings'],
      ['g p', 'Go to performance'],
      ['g t', 'Go to traces'],
      ['g l', 'Go to logs'],
      ['g m', 'Go to metrics'],
      ['g d', 'Go to dashboards'],
      ['g o', 'Go to profiles'],
      ['g v', 'Go to session replays'],
    ],
  },
  {
    group: 'Issue stream',
    items: [
      ['j / k', 'Move down / up'],
      ['x', 'Select issue'],
      ['shift x', 'Select range'],
      ['Enter / o', 'Open issue'],
      ['e', 'Resolve'],
      ['i', 'Ignore (archive)'],
      ['a', 'Assign to me'],
      ['b', 'Bookmark'],
      ['u', 'Mark unresolved'],
      ['Esc', 'Clear selection'],
    ],
  },
  {
    group: 'Issue details',
    items: [
      ['e', 'Resolve or unresolve'],
      ['i', 'Ignore or unignore'],
      ['a', 'Assign to me'],
      ['b', 'Bookmark'],
      ['h / l', 'Previous / next event'],
      ['shift h / shift l', 'Oldest / latest event'],
      ['c', 'Copy short id'],
      ['Esc', 'Back to the stream'],
    ],
  },
  {
    group: 'Trace view',
    items: [
      ['j / k', 'Next / previous span'],
      ['← / →', 'Collapse / expand'],
      ['Enter', 'Open span details'],
      ['c', 'Critical path'],
      ['e', 'Expand all'],
      ['= / -', 'Zoom in / out'],
      ['0', 'Reset zoom'],
    ],
  },
  {
    group: 'Logs',
    items: [
      ['j / k', 'Next / previous line'],
      ['Enter', 'Expand or collapse'],
      ['t', 'Live tail on or off'],
      ['/', 'Focus the query'],
    ],
  },
  {
    group: 'Flamegraph & replay',
    items: [
      ['/', 'Search frames'],
      ['0', 'Reset flamegraph zoom'],
      ['space', 'Play or pause the replay'],
      ['← / →', 'Back / forward 5 s'],
      [', / .', 'Slower / faster'],
      ['f', 'Fullscreen'],
    ],
  },
]

export function ShortcutsDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription>Single-key shortcuts work when no text field has focus.</DialogDescription>
        </DialogHeader>
        <div className="grid max-h-[70vh] gap-6 overflow-y-auto pb-2 sm:grid-cols-3">
          {SHORTCUTS.map((g) => (
            <section key={g.group}>
              <h3 className="mb-2 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{g.group}</h3>
              <dl className="space-y-1.5">
                {g.items.map(([keys, label]) => (
                  <div key={label} className="flex items-center justify-between gap-3 text-[13px]">
                    <dt className="text-foreground/90">{label}</dt>
                    <dd className="flex shrink-0 gap-1">
                      {keys.split(' ').map((k, i) =>
                        k === '/' && keys.length > 1 ? (
                          <span key={i} className="text-muted-foreground">
                            /
                          </span>
                        ) : (
                          <Kbd key={i}>{k}</Kbd>
                        ),
                      )}
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  )
}
