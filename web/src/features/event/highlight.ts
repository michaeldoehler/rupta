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

/** Syntax colours in the rupta palette: lime, palm, sand and coral on charcoal. */
const LIME_HAZE = {
  name: 'lime-haze',
  type: 'dark' as const,
  colors: { 'editor.background': '#12140d', 'editor.foreground': '#dfe5d2' },
  settings: [
    { settings: { foreground: '#dfe5d2' } },
    { scope: ['comment', 'punctuation.definition.comment'], settings: { foreground: '#7c8471', fontStyle: 'italic' } },
    { scope: ['keyword', 'storage', 'storage.type', 'storage.modifier', 'keyword.control', 'keyword.operator.new'], settings: { foreground: '#caf534' } },
    { scope: ['string', 'string.quoted', 'string.template', 'punctuation.definition.string'], settings: { foreground: '#a5c172' } },
    { scope: ['constant.numeric', 'constant.language', 'constant.character', 'support.constant'], settings: { foreground: '#f0a77c' } },
    { scope: ['entity.name.function', 'support.function', 'meta.function-call entity.name.function'], settings: { foreground: '#e6d9a8' } },
    { scope: ['entity.name.type', 'entity.name.class', 'support.class', 'support.type', 'entity.other.inherited-class'], settings: { foreground: '#9fd0c4' } },
    { scope: ['variable.parameter', 'variable.language', 'variable.other.readwrite.instance'], settings: { foreground: '#f0a77c' } },
    { scope: ['entity.name.tag', 'entity.other.attribute-name'], settings: { foreground: '#caf534' } },
    { scope: ['punctuation', 'meta.brace', 'keyword.operator'], settings: { foreground: '#a9b09a' } },
    { scope: ['invalid'], settings: { foreground: '#ee6a58' } },
  ],
}

let highlighterPromise: Promise<HighlighterCore> | null = null
const loaded = new Set<Lang>()

async function getHighlighter(): Promise<HighlighterCore> {
  if (!highlighterPromise) {
    highlighterPromise = (async () => {
      const [{ createHighlighterCore }, { createJavaScriptRegexEngine }] = await Promise.all([import('shiki/core'), import('shiki/engine/javascript')])
      return createHighlighterCore({ themes: [LIME_HAZE], langs: [], engine: createJavaScriptRegexEngine({ forgiving: true }) })
    })()
  }
  return highlighterPromise
}

export interface Token {
  content: string
  color?: string
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
  const result = hl.codeToTokens(prefix + lines.join('\n'), { lang, theme: 'lime-haze' }).tokens
  const out = result.map((line) => line.map((t) => ({ content: t.content, color: t.color })))
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
