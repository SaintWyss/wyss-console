import { describe, expect, it } from 'vitest';
import { visible } from '../src/lib/features/f8/present';

describe('visible', () => {
  it('shows bidirectional and invisible characters as code points', () => {
    expect(visible('rm ‮gpj.txt')).toBe('rm ⟨U+202E⟩gpj.txt');
    expect(visible('a​b⁦c')).toBe('a⟨U+200B⟩b⟨U+2066⟩c');
    expect(visible('line two')).toBe('line⟨U+2028⟩two');
  });
  it('leaves ordinary text, including non-Latin letters, alone', () => {
    expect(visible('/home/ñandú/проект/файл.ts')).toBe(
      '/home/ñandú/проект/файл.ts',
    );
  });
});
