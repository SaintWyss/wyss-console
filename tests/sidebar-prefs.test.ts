import { describe, expect, it } from 'vitest';
import { parseSidebar } from '../src/lib/shell/sidebar-prefs';

describe('sidebar preferences', () => {
  it('starts open and expanded when nothing is stored', () => {
    expect(parseSidebar(null)).toEqual({ rail: false, closed: [] });
  });
  it('reads a stored record and drops unknown groups', () => {
    expect(
      parseSidebar('{"rail":true,"closed":["agents","nope","system"]}'),
    ).toEqual({ rail: true, closed: ['agents', 'system'] });
  });
  it('falls back to the default on damaged data', () => {
    expect(parseSidebar('{not json')).toEqual({ rail: false, closed: [] });
    expect(parseSidebar('42')).toEqual({ rail: false, closed: [] });
    expect(parseSidebar('{"rail":"yes","closed":7}')).toEqual({
      rail: false,
      closed: [],
    });
  });
});
