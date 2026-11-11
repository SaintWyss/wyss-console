/**
 * Marks an English sentence as text the window shows, without translating it. Used where a
 * sentence is written outside a component (a label table, a presenter): the sentence stays the
 * value, so everything that reads it is unchanged, and `t()` translates it where it is drawn.
 * The dictionary test reads these calls to know every sentence that must have a translation.
 */
export const msg = <T extends string>(text: T): T => text;

/**
 * A sentence a presenter hands to a view: a `msg()` sentence and the values its places take.
 * The view draws it with `t(text, values)`, so every part is translated where it is drawn.
 * A place can take another phrase, drawn the same way, so a sentence can carry a sentence.
 */
export interface Phrase {
  text: string;
  values?: Record<string, string | number | Phrase>;
}
