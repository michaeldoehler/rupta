import { describe, expect, it } from 'vitest'
import { bytes, compact, duration, percent, relShort, shortVersion } from './format'

describe('format', () => {
  const now = new Date('2026-11-14T12:00:00Z')
  it('relShort covers past and future', () => {
    expect(relShort('2026-11-14T11:59:58Z', now)).toBe('now')
    expect(relShort('2026-11-14T11:59:00Z', now)).toBe('60s'.replace('60s', '1m'))
    expect(relShort('2026-11-14T09:00:00Z', now)).toBe('3h')
    expect(relShort('2026-11-10T12:00:00Z', now)).toBe('4d')
    expect(relShort('2026-11-14T12:05:00Z', now)).toBe('in 5m')
  })
  it('compact numbers', () => {
    expect(compact(999)).toBe('999')
    expect(compact(1234)).toBe('1.2k')
    expect(compact(250_000)).toBe('250k')
    expect(compact(3_400_000)).toBe('3.4M')
    expect(compact(null)).toBe('—')
  })
  it('percent keeps integers intact', () => {
    expect(percent(50, 0)).toBe('50%')
    expect(percent(0, 0)).toBe('0%')
    expect(percent(99.5, 2)).toBe('99.5%')
    expect(percent(100, 1)).toBe('100%')
  })
  it('bytes, duration and versions', () => {
    expect(bytes(512)).toBe('512 B')
    expect(bytes(3_400_000)).toBe('3.2 MiB')
    expect(duration(850)).toBe('850 ms')
    expect(duration(2470)).toBe('2.47 s')
    expect(shortVersion('web-app@1.3.8')).toBe('1.3.8')
    expect(shortVersion('api@2.0.0+abcdef123456')).toBe('2.0.0 (abcdef1)')
  })
})
