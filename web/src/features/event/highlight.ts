// Lazy syntax highlighting with shiki: the highlighter, grammars and themes
// load on first use in their own chunks, so the issue stream never pays
// for them.

import { useEffect, useState } from 'react'
import type { HighlighterCore } from 'shiki/core'

export type Lang =
  'javascript' | 'typescript' | 'tsx' | 'python' | 'php' | 'go' | 'ruby' | 'java' | 'csharp' | 'rust' | 'bash' | 'json' | 'yaml' | 'toml' | 'xml'

const LOADERS: Record<Lang, () => Promise<unknown>> = {
  javascript: () => import('shiki/dist/langs/javascript.mjs'),
  typescript: () => import('shiki/dist/langs/typescript.mjs'),
  tsx: () => import('shiki/dist/langs/tsx.mjs'),
  python: () => import('shiki/dist/langs/python.mjs'),
  php: () => import('shiki/dist/langs/php.mjs'),
  go: () => import('shiki/dist/langs/go.mjs'),
  ruby: () => import('shiki/dist/langs/ruby.mjs'),
  java: () => import('shiki/dist/langs/java.mjs'),
  csharp: () => import('shiki/dist/langs/csharp.mjs'),
  rust: () => import('shiki/dist/langs/rust.mjs'),
  bash: () => import('shiki/dist/langs/bash.mjs'),
  json: () => import('shiki/dist/langs/json.mjs'),
  yaml: () => import('shiki/dist/langs/yaml.mjs'),
  toml: () => import('shiki/dist/langs/toml.mjs'),
  xml: () => import('shiki/dist/langs/xml.mjs'),
}

export function detectLang(filename: string | null | undefined, platform?: string | null): Lang | null {
  const f = (filename ?? '').toLowerCase().split('?')[0]
  const ext = f.slice(f.lastIndexOf('.') + 1)
  const byExt: Record<string, Lang> = {
    js: 'javascript',
    mjs: 'javascript',
    cjs: 'javascript',
    jsx: 'tsx',
    ts: 'typescript',
    mts: 'typescript',
    tsx: 'tsx',
    py: 'python',
    php: 'php',
    go: 'go',
    rb: 'ruby',
    java: 'java',
    kt: 'java',
    cs: 'csharp',
    rs: 'rust',
  }
  if (byExt[ext]) return byExt[ext]
  const byPlatform: Record<string, Lang> = {
    javascript: 'javascript',
    node: 'javascript',
    python: 'python',
    php: 'php',
    go: 'go',
    ruby: 'ruby',
    java: 'java',
    csharp: 'csharp',
    rust: 'rust',
  }
  return platform ? (byPlatform[platform] ?? null) : null
}

let highlighterPromise: Promise<HighlighterCore> | null = null
const loaded = new Set<Lang>()

async function getHighlighter(): Promise<HighlighterCore> {
  if (!highlighterPromise) {
    highlighterPromise = (async () => {
      const [{ createHighlighterCore }, { createJavaScriptRegexEngine }, light, dark] = await Promise.all([
        import('shiki/core'),
        import('shiki/engine/javascript'),
        import('shiki/dist/themes/github-light.mjs'),
        import('shiki/dist/themes/github-dark.mjs'),
      ])
      return createHighlighterCore({ themes: [light.default, dark.default], langs: [], engine: createJavaScriptRegexEngine({ forgiving: true }) })
    })()
  }
  return highlighterPromise
}

export interface Token {
  content: string
  light?: string
  dark?: string
}

export async function highlightLines(lines: string[], lang: Lang): Promise<Token[][]> {
  const hl = await getHighlighter()
  if (!loaded.has(lang)) {
    const mod = (await LOADERS[lang]()) as { default: Parameters<HighlighterCore['loadLanguage']>[0] }
    await hl.loadLanguage(mod.default)
    loaded.add(lang)
  }
  // PHP snippets lack the opening tag; the grammar needs it.
  const prefix = lang === 'php' && !lines.some((l) => l.includes('<?php')) ? '<?php\n' : ''
  const result = hl.codeToTokensWithThemes(prefix + lines.join('\n'), { lang, themes: { light: 'github-light', dark: 'github-dark' } })
  const out = result.map((line) => line.map((t) => ({ content: t.content, light: t.variants.light?.color, dark: t.variants.dark?.color })))
  return prefix ? out.slice(1) : out
}

/** Highlighted tokens per line, or null while loading / unsupported. */
export function useHighlight(lines: string[], lang: Lang | null, enabled = true): Token[][] | null {
  const key = lines.join('\n')
  const [state, setState] = useState<{ key: string; tokens: Token[][] } | null>(null)
  useEffect(() => {
    if (!lang || !enabled || !key) return
    let cancelled = false
    highlightLines(key.split('\n'), lang)
      .then((tokens) => {
        if (!cancelled) setState({ key, tokens })
      })
      .catch(() => {
        /* fall back to plain text */
      })
    return () => {
      cancelled = true
    }
  }, [key, lang, enabled])
  return state?.key === key ? state.tokens : null
}
