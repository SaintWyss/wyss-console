import { mount } from 'svelte';
import App from './App.svelte';
import ProjectsWindow from './lib/features/projects/ProjectsWindow.svelte';
import './app.css';

const target = document.getElementById('app');
if (!target) throw new Error('Application root is missing');

// A window opened for the project workspace carries one capability and shows one view.
mount(location.hash === '#workspace' ? ProjectsWindow : App, { target });
