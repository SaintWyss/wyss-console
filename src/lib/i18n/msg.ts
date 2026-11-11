/**
 * Marks an English sentence as text the window shows, without translating it. Used where a
 * sentence is written outside a component (a label table, a presenter): the sentence stays the
 * value, so everything that reads it is unchanged, and `t()` translates it where it is drawn.
 * The dictionary test reads these calls to know every sentence that must have a translation.
 */
export const msg = <T extends string>(text: T): T => text;
