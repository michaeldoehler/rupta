import { http, HttpResponse } from 'msw'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { server } from '@/mocks/node'
import { ApiError, buildQuery, errorMessage, onUnauthorized, request, requestList, setCsrfToken } from './client'

afterEach(() => setCsrfToken(null))

describe('buildQuery', () => {
  it('joins arrays with commas, repeats listed keys and drops empty values', () => {
    expect(buildQuery({ project: ['1', '2'], query: 'is:unresolved', empty: '', none: undefined, list: [] })).toBe('?project=1%2C2&query=is%3Aunresolved')
    expect(buildQuery({ id: ['1', '2'] }, ['id'])).toBe('?id=1&id=2')
    expect(buildQuery({})).toBe('')
  })
})

describe('request', () => {
  it('parses the error envelope of the API', async () => {
    server.use(
      http.get('/api/v1/thing', () =>
        HttpResponse.json({ error: { code: 'validation_failed', message: 'Bad name', details: { fields: { name: 'Too short' } } } }, { status: 422 }),
      ),
    )
    const err = await request('/thing').catch((e) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err).toMatchObject({ status: 422, code: 'validation_failed', message: 'Bad name' })
    expect((err as ApiError).fieldErrors).toEqual({ name: 'Too short' })
  })

  it('falls back to status-based errors for non-JSON bodies', async () => {
    server.use(http.get('/api/v1/thing', () => new HttpResponse('<html>Bad gateway</html>', { status: 502 })))
    const err = (await request('/thing').catch((e) => e)) as ApiError
    expect(err.code).toBe('bad_gateway')
    expect(err.message).toBe('The server is not reachable.')
  })

  it('reports network failures as status 0', async () => {
    server.use(http.get('/api/v1/thing', () => HttpResponse.error()))
    const err = (await request('/thing').catch((e) => e)) as ApiError
    expect(err.isNetwork).toBe(true)
    expect(errorMessage(err)).toMatch(/Cannot reach the server/)
  })

  it('notifies listeners on 401 unless quiet', async () => {
    server.use(http.get('/api/v1/thing', () => HttpResponse.json({ error: { code: 'unauthorized', message: 'No session' } }, { status: 401 })))
    const fn = vi.fn()
    const off = onUnauthorized(fn)
    await request('/thing').catch(() => undefined)
    await request('/thing', { quiet401: true }).catch(() => undefined)
    off()
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('sends the CSRF token on mutating requests only', async () => {
    const seen: (string | null)[] = []
    server.use(
      http.all('/api/v1/echo', ({ request: r }) => {
        seen.push(r.headers.get('x-csrf-token'))
        return new HttpResponse(null, { status: 204 })
      }),
    )
    setCsrfToken('tok')
    await request('/echo')
    await request('/echo', { method: 'POST', body: { a: 1 } })
    expect(seen).toEqual([null, 'tok'])
  })

  it('returns undefined for 204 and rejects invalid JSON', async () => {
    server.use(
      http.get('/api/v1/empty', () => new HttpResponse(null, { status: 204 })),
      http.get('/api/v1/broken', () => new HttpResponse('{oops', { status: 200 })),
    )
    expect(await request('/empty')).toBeUndefined()
    await expect(request('/broken')).rejects.toMatchObject({ code: 'invalid_response' })
  })

  it('requestList accepts arrays and pages', async () => {
    server.use(
      http.get('/api/v1/a', () => HttpResponse.json([1, 2])),
      http.get('/api/v1/b', () => HttpResponse.json({ data: [3], next_cursor: null })),
    )
    expect(await requestList('/a')).toEqual([1, 2])
    expect(await requestList('/b')).toEqual([3])
  })
})
