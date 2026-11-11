import js from '@eslint/js';
import globals from 'globals';
import svelte from 'eslint-plugin-svelte';
import ts from 'typescript-eslint';
import { defineConfig, globalIgnores } from 'eslint/config';
import svelteConfig from './svelte.config.js';

export default defineConfig([
  globalIgnores(['dist/**', 'src-tauri/**', '.cache/**', '.artifacts/**']),
  js.configs.recommended,
  ...ts.configs.recommended,
  ...svelte.configs.recommended,
  { languageOptions: { globals: { ...globals.browser, ...globals.node } } },
  {
    files: ['src/**/*.ts', 'src/**/*.svelte'],
    ignores: ['src/lib/desktop/**'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              group: ['@tauri-apps/**'],
              message: 'Use the dedicated desktop IPC client.',
            },
          ],
        },
      ],
    },
  },
  {
    files: ['**/*.svelte', '**/*.svelte.ts'],
    languageOptions: {
      parserOptions: { parser: ts.parser, svelteConfig },
    },
  },
]);
