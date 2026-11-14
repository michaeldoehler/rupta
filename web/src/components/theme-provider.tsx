import * as React from 'react'

// rupta has one dark theme. The provider keeps the old interface so
// components can ask for the theme; there is nothing to switch.
export type Theme = 'dark'

interface ThemeState {
  theme: Theme
  resolved: 'dark'
}

const VALUE: ThemeState = { theme: 'dark', resolved: 'dark' }
const ThemeContext = React.createContext<ThemeState>(VALUE)

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  React.useLayoutEffect(() => {
    document.documentElement.classList.add('dark')
    document.documentElement.style.colorScheme = 'dark'
  }, [])
  return <ThemeContext.Provider value={VALUE}>{children}</ThemeContext.Provider>
}

// eslint-disable-next-line react-refresh/only-export-components
export function useTheme(): ThemeState {
  return React.useContext(ThemeContext)
}
