import { msg } from '../i18n/msg';
import type { WorkspaceState } from '../features/terminal/session-manager';

export type RuntimeTone = 'ok' | 'pending' | 'warn' | 'error' | 'idle';

export interface RuntimeSummary {
  label: string;
  tone: RuntimeTone;
}

export const idleRuntime: RuntimeSummary = {
  label: msg('No terminals'),
  tone: 'idle',
};

/** Presentation only: reduces the local terminal workspace to one top-bar status. */
export function summarizeRuntime(workspace: WorkspaceState): RuntimeSummary {
  if (workspace.error) return { label: msg('Error'), tone: 'error' };
  const active = workspace.sessions.find((s) => s.id === workspace.active);
  if (!active)
    return workspace.creating
      ? { label: msg('Starting'), tone: 'pending' }
      : idleRuntime;
  switch (active.view.state) {
    case 'running':
      return { label: msg('Running'), tone: 'ok' };
    case 'starting':
      return { label: msg('Starting'), tone: 'pending' };
    case 'closing':
      return { label: msg('Closing'), tone: 'pending' };
    case 'exited':
      return { label: msg('Exited'), tone: 'warn' };
    case 'closed':
      return { label: msg('Closed'), tone: 'idle' };
    case 'failed':
      return { label: msg('Failed'), tone: 'error' };
  }
}
