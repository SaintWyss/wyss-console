import { mount } from 'svelte';
import App from './App.svelte';
import ProjectsWindow from './lib/features/projects/ProjectsWindow.svelte';
import './app.css';

const target = document.getElementById('app');
if (!target) throw new Error('Application root is missing');

/** The terminal measures its cells once, with the font it has: the bundled mono font is loaded
 * first. Bounded, so a font that does not load never keeps the window from opening. */
async function fontsReady(): Promise<void> {
  const fonts = (document as Document & { fonts?: FontFaceSet }).fonts;
  if (!fonts) return;
  await Promise.race([
    Promise.all([
      fonts.load('400 14px "IBM Plex Mono"'),
      fonts.load('500 14px "IBM Plex Mono"'),
      fonts.load('400 14px "IBM Plex Sans"'),
    ]).catch(() => undefined),
    new Promise((resolve) => setTimeout(resolve, 1500)),
  ]);
}

// A window opened for the project workspace carries one capability and shows one view.
void fontsReady().then(() =>
  mount(location.hash === '#workspace' ? ProjectsWindow : App, { target }),
);
