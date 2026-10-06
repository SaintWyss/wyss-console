import { afterEach } from 'vitest';
import { clearMocks } from '@tauri-apps/api/mocks';

afterEach(() => {
  clearMocks();
  document.body.replaceChildren();
});
