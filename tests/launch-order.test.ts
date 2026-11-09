import { describe, expect, it } from 'vitest';
import {
  applyOrder,
  moveLaunch,
  parseOrder,
} from '../src/lib/features/terminal/launch-order';

const items = ['a', 'b', 'c', 'd'].map((id) => ({ id }));

describe('launch order', () => {
  it('keeps the given order when nothing is stored', () => {
    expect(applyOrder(items, []).map((i) => i.id)).toEqual([
      'a',
      'b',
      'c',
      'd',
    ]);
  });
  it('puts the named launches first and the new ones after, in their order', () => {
    expect(applyOrder(items, ['c', 'a']).map((i) => i.id)).toEqual([
      'c',
      'a',
      'b',
      'd',
    ]);
  });
  it('ignores ids that no longer exist', () => {
    expect(applyOrder(items, ['zzz', 'b']).map((i) => i.id)).toEqual([
      'b',
      'a',
      'c',
      'd',
    ]);
  });
  it('moves one place and stops at the ends', () => {
    expect(moveLaunch(['a', 'b', 'c'], 'b', -1)).toEqual(['b', 'a', 'c']);
    expect(moveLaunch(['a', 'b', 'c'], 'b', 1)).toEqual(['a', 'c', 'b']);
    expect(moveLaunch(['a', 'b', 'c'], 'a', -1)).toEqual(['a', 'b', 'c']);
    expect(moveLaunch(['a', 'b', 'c'], 'c', 1)).toEqual(['a', 'b', 'c']);
  });
  it('reads damaged storage as no order', () => {
    expect(parseOrder('{oops')).toEqual([]);
    expect(parseOrder('{"a":1}')).toEqual([]);
    expect(parseOrder('["a",3,"b"]')).toEqual(['a', 'b']);
  });
});
