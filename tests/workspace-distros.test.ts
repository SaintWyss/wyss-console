import { expect, it } from 'vitest';
import {
  compose,
  DISTROS,
  distroOf,
  fresh,
  linesOf,
} from '../src/lib/features/workspace/distros';

it('the shipped distributions are the five agreed, with no confirmation switched off', () => {
  expect(DISTROS.map((d) => d.id)).toEqual([
    'cachyos',
    'arch',
    'debian',
    'fedora',
    'opensuse',
  ]);
  for (const d of DISTROS)
    expect(fresh(d).commands.join(' ')).not.toMatch(/noconfirm| -y\b/);
  expect(fresh(distroOf('cachyos')!).commands).toEqual([
    'sudo pacman -Syu',
    'paru -Sua',
  ]);
});

it('switches add their lines and flags in the lines’ order', () => {
  const cachy = distroOf('cachyos')!;
  // The owner's sysupdate, rebuilt from switches.
  expect(compose(cachy, ['noconfirm', 'aur', 'skipreview'])).toEqual([
    'sudo pacman -Syu --noconfirm',
    'paru -Sua --noconfirm --skipreview',
  ]);
  expect(compose(cachy, ['clean'])).toEqual([
    'sudo pacman -Syu',
    'sudo paccache -rk2',
  ]);
  expect(
    compose(distroOf('debian')!, ['yes', 'autoremove', 'flatpak']),
  ).toEqual([
    'sudo apt update',
    'sudo apt full-upgrade -y',
    'sudo apt autoremove -y',
    'flatpak update -y',
  ]);
  expect(compose(distroOf('opensuse')!, ['yes'])).toEqual([
    'sudo zypper refresh',
    'sudo zypper dup -y',
  ]);
});

it('a typed script is one command per non-empty line', () => {
  expect(linesOf('  a \n\n b\n')).toEqual(['a', 'b']);
});
