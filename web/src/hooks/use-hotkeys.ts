import * as React from 'react'

export function isTypingTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false
  const tag = el.tagName
  return (
    tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable || el.closest('[role="dialog"],[role="menu"],[role="listbox"]') !== null
  )
}

export type HotkeyMap = Record<string, (e: KeyboardEvent) => void>

/**
 * Global single-key shortcuts ("j", "shift+e", "mod+k", "g i" sequences).
 * Ignored while typing in inputs or inside dialogs/menus unless the key
 * includes "mod".
 */
export function useHotkeys(map: HotkeyMap, deps: React.DependencyList = [], enabled = true) {
  const mapRef = React.useRef(map)
  React.useEffect(() => {
    mapRef.current = map
  })
  React.useEffect(() => {
    if (!enabled) return
    let pending: string | null = null
    let pendingTimer: ReturnType<typeof setTimeout> | undefined
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing || e.defaultPrevented) return
      const mod = e.metaKey || e.ctrlKey
      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key
      const combo = `${mod ? 'mod+' : ''}${e.shiftKey && e.key.length !== 1 ? 'shift+' : ''}${e.altKey ? 'alt+' : ''}${key}`
      const typing = isTypingTarget(e.target)
      if (typing && !mod) return
      const handlers = mapRef.current
      if (pending) {
        const seq = `${pending} ${key}`
        pending = null
        clearTimeout(pendingTimer)
        if (handlers[seq]) {
          e.preventDefault()
          handlers[seq](e)
          return
        }
      }
      const shiftCombo = e.shiftKey && e.key.length === 1 ? `shift+${key}` : null
      const handler = (shiftCombo && handlers[shiftCombo]) || handlers[combo] || (e.key === '?' ? handlers['?'] : undefined)
      if (handler) {
        e.preventDefault()
        handler(e)
        return
      }
      if (!mod && Object.keys(handlers).some((k) => k.startsWith(`${key} `))) {
        pending = key
        pendingTimer = setTimeout(() => (pending = null), 900)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('keydown', onKey)
      clearTimeout(pendingTimer)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, ...deps])
}
