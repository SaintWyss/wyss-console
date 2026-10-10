import type { LifecycleView } from '../src/lib/contracts/lifecycle';

/** A view as the desktop sends it. Checking, with nothing known, unless said otherwise. */
export function lifecycle(over: Partial<LifecycleView> = {}): LifecycleView {
  const phase = over.phase ?? 'checking';
  return {
    schema_version: 1,
    revision: 1,
    phase,
    failure: null,
    retry: phase === 'failed',
    origin: null,
    update_pending: false,
    diagnostics: {
      build: 'packaged',
      installation: 'unknown',
      service: 'unknown',
    },
    ...over,
  };
}
