import { createContext, useContext } from 'react'

export interface ShellState {
  openPalette: (initial?: string) => void
  openShortcuts: () => void
}

export const ShellContext = createContext<ShellState>({ openPalette: () => {}, openShortcuts: () => {} })

export function useShell() {
  return useContext(ShellContext)
}
