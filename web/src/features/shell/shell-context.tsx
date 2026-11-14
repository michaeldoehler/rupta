import { createContext, useContext } from 'react'

export interface ShellState {
  openPalette: (initial?: string) => void
  openShortcuts: () => void
  toggleSidebar: () => void
}

export const ShellContext = createContext<ShellState>({ openPalette: () => {}, openShortcuts: () => {}, toggleSidebar: () => {} })

export function useShell() {
  return useContext(ShellContext)
}
