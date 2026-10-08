import type { WorkspaceState } from '../features/terminal/session-manager';

export type RuntimeTone = 'ok' | 'pending' | 'warn' | 'error' | 'idle';

export interface RuntimeSummary {
  label: string;
  tone: RuntimeTone;
}

export const idleRuntime: RuntimeSummary = {
  label: 'No terminals',
  tone: 'idle',
};

/** Presentation only: reduces the local terminal workspace to one top-bar status. */
export function summarizeRuntime(workspace: WorkspaceState): RuntimeSummary {
  if (workspace.error) return { label: 'Error', tone: 'error' };
  const active = workspace.sessions.find((s) => s.id === workspace.active);
  if (!active)
    return workspace.creating
      ? { label: 'Starting', tone: 'pending' }
      : idleRuntime;
  switch (active.view.state) {
    case 'running':
      return { label: 'Running', tone: 'ok' };
    case 'starting':
      return { label: 'Starting', tone: 'pending' };
    case 'closing':
      return { label: 'Closing', tone: 'pending' };
    case 'exited':
      return { label: 'Exited', tone: 'warn' };
    case 'closed':
      return { label: 'Closed', tone: 'idle' };
    case 'failed':
      return { label: 'Failed', tone: 'error' };
  }
}
