import { invoke } from '@tauri-apps/api/core';
import {
  parseAck,
  parseM3State,
  type Ack,
  type Decision,
  type M3State,
} from '../contracts/m3';

/** What the window can ask of the M3 service: one read, and the fixture runtime's few actions. */
export interface M3Client {
  state(): Promise<M3State>;
  fixtureStart(profile: string): Promise<Ack>;
  fixtureStop(): Promise<Ack>;
  fixturePrompt(text: string): Promise<Ack>;
  fixtureInterrupt(): Promise<Ack>;
  fixtureDecide(item: string, decision: Decision): Promise<Ack>;
}

export const desktopM3: M3Client = {
  state: async () => parseM3State(await invoke<unknown>('m3_state')),
  fixtureStart: async (profile) =>
    parseAck(await invoke<unknown>('m3_fixture_start', { profile })),
  fixtureStop: async () => parseAck(await invoke<unknown>('m3_fixture_stop')),
  fixturePrompt: async (text) =>
    parseAck(await invoke<unknown>('m3_fixture_prompt', { text })),
  fixtureInterrupt: async () =>
    parseAck(await invoke<unknown>('m3_fixture_interrupt')),
  fixtureDecide: async (item, decision) =>
    parseAck(await invoke<unknown>('m3_fixture_decide', { item, decision })),
};
