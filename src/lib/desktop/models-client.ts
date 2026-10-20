import { invoke } from '@tauri-apps/api/core';
import { parseModelCatalogue, type ModelCatalogue } from '../contracts/models';
import {
  parseSessionFailure,
  type SessionFailure,
} from '../contracts/sessions';

/** A catalogue read the desktop refused or could not complete, in product terms. */
export class CatalogueCallError extends Error {
  constructor(readonly failure: SessionFailure) {
    super(`catalogue: ${failure.code}`);
  }
}

/**
 * One read, with no argument: the window cannot name a runtime, a model or a page. It is
 * told the catalogue the authority holds, or that the runtime is not ready.
 */
export async function readModelCatalogue(): Promise<ModelCatalogue> {
  let answer: unknown;
  try {
    answer = await invoke<unknown>('managed_model_catalogue');
  } catch (error) {
    throw new CatalogueCallError(parseSessionFailure(error));
  }
  try {
    return parseModelCatalogue(answer);
  } catch {
    // An answer that is not the projection is a failed bridge, never data.
    throw new CatalogueCallError({ code: 'internal', retry: false });
  }
}
