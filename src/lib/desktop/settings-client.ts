import { invoke } from '@tauri-apps/api/core';
import {
  parsePreferences,
  parseSettingsFailure,
  parseSettingsView,
  type Preferences,
  type SettingsFailureCode,
  type SettingsView,
} from '../contracts/settings';

/**
 * The settings calls the window can make: read, replace the whole typed set, reset.
 * None names a key, a path, a file or a program. The desktop judges every value again.
 */
export class SettingsCallError extends Error {
  constructor(readonly code: SettingsFailureCode) {
    super(`settings: ${code}`);
  }
}

async function call(
  command: string,
  request: Record<string, unknown>,
): Promise<SettingsView> {
  let answer: unknown;
  try {
    answer = await invoke<unknown>(command, { request });
  } catch (error) {
    throw new SettingsCallError(parseSettingsFailure(error));
  }
  try {
    return parseSettingsView(answer);
  } catch {
    // The desktop answered something that is not a view: nothing is believed.
    throw new SettingsCallError('internal');
  }
}

export function readSettings(): Promise<SettingsView> {
  return call('settings_read', {});
}

/** Replaces the preferences with `preferences`, whole. The request is checked first. */
export function updateSettings(
  preferences: Preferences,
): Promise<SettingsView> {
  let checked: Preferences;
  try {
    checked = parsePreferences(preferences);
  } catch {
    return Promise.reject(new SettingsCallError('invalid'));
  }
  return call('settings_update', { preferences: checked });
}

export function resetSettings(): Promise<SettingsView> {
  return call('settings_reset', {});
}
