import { msg } from '../../i18n/index.svelte';
import type { Variant } from '../../desktop/launch-client';

/**
 * The system update's distributions: the lines each one runs and the switches that change them.
 * A line with `requires` appears only with that switch on; a switch's `flags` are appended to the
 * lines they name. The desktop keeps what the window composes; it never builds a script itself.
 */
export interface Line {
  key: string;
  text: string;
  requires?: string;
}
export interface Switch {
  id: string;
  label: string;
  flags?: Readonly<Record<string, string>>;
}
export interface Distro {
  id: string;
  label: string;
  lines: readonly Line[];
  switches: readonly Switch[];
}

const FLATPAK: Line = {
  key: 'flatpak',
  text: 'flatpak update',
  requires: 'flatpak',
};
const FLATPAK_SWITCH: Switch = {
  id: 'flatpak',
  label: msg('Update Flatpak apps'),
};
const PACMAN: readonly Line[] = [
  { key: 'pacman', text: 'sudo pacman -Syu' },
  { key: 'paru', text: 'paru -Sua', requires: 'aur' },
  { key: 'clean', text: 'sudo paccache -rk2', requires: 'clean' },
  FLATPAK,
];
const pacmanSwitches: readonly Switch[] = [
  {
    id: 'noconfirm',
    label: msg('No confirmation (--noconfirm)'),
    flags: { pacman: '--noconfirm', paru: '--noconfirm', flatpak: '-y' },
  },
  { id: 'aur', label: msg('Include AUR (paru)') },
  {
    id: 'skipreview',
    label: msg('Skip AUR review (--skipreview)'),
    flags: { paru: '--skipreview' },
  },
  { id: 'clean', label: msg('Clean the package cache (paccache -rk2)') },
  FLATPAK_SWITCH,
];

export const DISTROS: readonly Distro[] = [
  {
    id: 'cachyos',
    label: 'CachyOS',
    lines: PACMAN,
    switches: pacmanSwitches,
  },
  { id: 'arch', label: 'Arch', lines: PACMAN, switches: pacmanSwitches },
  {
    id: 'debian',
    label: 'Ubuntu / Debian',
    lines: [
      { key: 'update', text: 'sudo apt update' },
      { key: 'upgrade', text: 'sudo apt full-upgrade' },
      {
        key: 'autoremove',
        text: 'sudo apt autoremove',
        requires: 'autoremove',
      },
      FLATPAK,
    ],
    switches: [
      {
        id: 'yes',
        label: msg('No confirmation (-y)'),
        flags: { upgrade: '-y', autoremove: '-y', flatpak: '-y' },
      },
      { id: 'autoremove', label: msg('Remove unused packages (autoremove)') },
      FLATPAK_SWITCH,
    ],
  },
  {
    id: 'fedora',
    label: 'Fedora',
    lines: [
      { key: 'dnf', text: 'sudo dnf upgrade --refresh' },
      {
        key: 'autoremove',
        text: 'sudo dnf autoremove',
        requires: 'autoremove',
      },
      FLATPAK,
    ],
    switches: [
      {
        id: 'yes',
        label: msg('No confirmation (-y)'),
        flags: { dnf: '-y', autoremove: '-y', flatpak: '-y' },
      },
      { id: 'autoremove', label: msg('Remove unused packages (autoremove)') },
      FLATPAK_SWITCH,
    ],
  },
  {
    id: 'opensuse',
    label: 'openSUSE',
    lines: [
      { key: 'refresh', text: 'sudo zypper refresh' },
      { key: 'dup', text: 'sudo zypper dup' },
      FLATPAK,
    ],
    switches: [
      {
        id: 'yes',
        label: msg('No confirmation (-y)'),
        flags: { dup: '-y', flatpak: '-y' },
      },
      FLATPAK_SWITCH,
    ],
  },
];

export const distroOf = (id: string) => DISTROS.find((d) => d.id === id);

/** The commands a distribution's lines make with these switches on, in the lines' order. */
export function compose(distro: Distro, on: readonly string[]): string[] {
  const enabled = new Set(on);
  return distro.lines
    .filter((line) => !line.requires || enabled.has(line.requires))
    .map((line) => {
      const flags = distro.switches
        .filter((s) => enabled.has(s.id) && s.flags?.[line.key])
        .map((s) => s.flags![line.key]);
      return [line.text, ...flags].join(' ');
    });
}

/** A variant as its distribution's defaults make it: nothing switched on but AUR on CachyOS. */
export function fresh(distro: Distro): Variant {
  const options = distro.id === 'cachyos' ? ['aur'] : [];
  return {
    distro: distro.id,
    options,
    manual: false,
    commands: compose(distro, options),
  };
}

/** The script as typed in the editor: one command per non-empty line. */
export const linesOf = (script: string) =>
  script
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l !== '');

/** An os-release ID: what a variant is saved under. */
export const distroIdOk = (id: string) => /^[a-z0-9_-]{1,64}$/.test(id);
