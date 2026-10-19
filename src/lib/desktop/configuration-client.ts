import { invoke } from '@tauri-apps/api/core';
import {
  parseChanges,
  parseConfigurationFailure,
  parseInventoryReply,
  type Changes,
  type ConfigurationFailure,
  type InventoryReply,
} from '../contracts/configuration';

/**
 * The two reads of the agents' configuration the window can make. Neither names a path or
 * a file; both answer from what the desktop already holds, so asking often costs the
 * providers' files nothing.
 */
export class ConfigurationCallError extends Error {
  constructor(readonly code: ConfigurationFailure) {
    super(`configuration: ${code}`);
  }
}

async function call(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  try {
    return await invoke<unknown>(command, args);
  } catch (error) {
    throw new ConfigurationCallError(parseConfigurationFailure(error));
  }
}

export async function readConfigurationInventory(): Promise<InventoryReply> {
  const answer = await call('configuration_inventory', {});
  try {
    return parseInventoryReply(answer);
  } catch {
    // The desktop answered something that is not an inventory: nothing is believed.
    throw new ConfigurationCallError('internal');
  }
}

export async function readConfigurationChanges(
  since: number,
): Promise<Changes> {
  if (!Number.isSafeInteger(since) || since < 0)
    throw new ConfigurationCallError('internal');
  const answer = await call('configuration_changes', { since });
  try {
    return parseChanges(answer);
  } catch {
    throw new ConfigurationCallError('internal');
  }
}
