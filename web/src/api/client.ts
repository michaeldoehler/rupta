// The single place that talks HTTP to the rupta server. Everything else goes
// through `api` (endpoints.ts), so adapting to server changes happens here.

import type { ApiErrorBody } from './types'

export const API_BASE = '/api/v1'

export class ApiError extends Error {
  readonly status: number
  readonly code: string
  readonly details: Record<string, unknown>

  constructor(status: number, code: string, message: string, details: Record<string, unknown> = {}) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.details = details
  }

  get isNotFound() {
    return this.status === 404
  }
  get isUnauthorized() {
    return this.status === 401
  }
  get isForbidden() {
    return this.status === 403
  }
  get isNetwork() {
    return this.status === 0
  }
  /** Field errors from validation failures: `details.fields = {name: msg}`. */
  get fieldErrors(): Record<string, string> {
    const f = this.details.fields
    return f && typeof f === 'object' ? (f as Record<string, string>) : {}
  }
}

const STATUS_CODES: Record<number, string> = {
  400: 'bad_request',
  401: 'unauthorized',
  403: 'forbidden',
  404: 'not_found',
  409: 'conflict',
  422: 'validation_failed',
  429: 'rate_limited',
  500: 'internal',
  502: 'bad_gateway',
  503: 'unavailable',
}

const STATUS_MESSAGES: Record<number, string> = {
  401: 'You are not signed in.',
  403: 'You do not have permission to do this.',
  404: 'Not found.',
  429: 'Too many requests. Try again in a moment.',
  502: 'The server is not reachable.',
  503: 'The server is temporarily unavailable.',
}

let csrfToken: string | null = null
export function setCsrfToken(token: string | null) {
  csrfToken = token
}

type UnauthorizedListener = () => void
const unauthorizedListeners = new Set<UnauthorizedListener>()
/** Called whenever a request (other than the session probe) gets a 401. */
export function onUnauthorized(fn: UnauthorizedListener) {
  unauthorizedListeners.add(fn)
  return () => unauthorizedListeners.delete(fn)
}

export type QueryValue = string | number | boolean | null | undefined | (string | number)[]

/**
 * Builds a query string. Arrays are comma-joined (`project=1,2`) unless the
 * key is listed in `repeat`, in which case the key is repeated (`id=1&id=2`).
 * Empty values are left out.
 */
export function buildQuery(params: Record<string, QueryValue> | undefined, repeat: string[] = []): string {
  if (!params) return ''
  const sp = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === '') continue
    if (Array.isArray(value)) {
      if (value.length === 0) continue
      if (repeat.includes(key)) value.forEach((v) => sp.append(key, String(v)))
      else sp.set(key, value.join(','))
    } else {
      sp.set(key, String(value))
    }
  }
  const s = sp.toString()
  return s ? `?${s}` : ''
}

export interface RequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'
  query?: Record<string, QueryValue>
  repeat?: string[]
  body?: unknown
  signal?: AbortSignal
  /** Do not notify unauthorized listeners on 401 (session probe, login). */
  quiet401?: boolean
}

async function parseError(res: Response): Promise<ApiError> {
  let body: unknown
  try {
    const text = await res.text()
    body = text ? JSON.parse(text) : null
  } catch {
    body = null
  }
  const err = (body as ApiErrorBody | null)?.error
  if (err && typeof err.code === 'string') {
    return new ApiError(res.status, err.code, err.message || STATUS_MESSAGES[res.status] || res.statusText, err.details ?? {})
  }
  return new ApiError(
    res.status,
    STATUS_CODES[res.status] ?? (res.status >= 500 ? 'internal' : 'error'),
    STATUS_MESSAGES[res.status] ?? `Request failed (${res.status}${res.statusText ? ` ${res.statusText}` : ''}).`,
  )
}

export async function request<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const method = opts.method ?? 'GET'
  const headers: Record<string, string> = { Accept: 'application/json' }
  if (opts.body !== undefined) headers['Content-Type'] = 'application/json'
  if (method !== 'GET' && csrfToken) headers['X-CSRF-Token'] = csrfToken

  const url = `${API_BASE}${path}${buildQuery(opts.query, opts.repeat)}`
  let res: Response
  try {
    res = await fetch(url, {
      method,
      headers,
      credentials: 'same-origin',
      body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
      signal: opts.signal,
    })
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') throw e
    throw new ApiError(0, 'network_error', 'Cannot reach the server. Check your connection.')
  }

  if (!res.ok) {
    const err = await parseError(res)
    if (res.status === 401 && !opts.quiet401) unauthorizedListeners.forEach((fn) => fn())
    throw err
  }
  if (res.status === 204) return undefined as T
  const text = await res.text()
  if (!text) return undefined as T
  try {
    return JSON.parse(text) as T
  } catch {
    throw new ApiError(res.status, 'invalid_response', 'The server sent an invalid response.')
  }
}

/** Human-readable message for any thrown value. */
export function errorMessage(e: unknown): string {
  if (e instanceof ApiError) return e.message
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/**
 * For collection endpoints: accepts both a bare array and a page
 * (`{data, next_cursor}`), so small server differences do not break the UI.
 */
export async function requestList<T>(path: string, opts: RequestOptions = {}): Promise<T[]> {
  const body = await request<T[] | { data: T[] } | null | undefined>(path, opts)
  if (Array.isArray(body)) return body
  if (body && Array.isArray((body as { data: T[] }).data)) return (body as { data: T[] }).data
  return []
}
