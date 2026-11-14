import js from '@eslint/js'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import globals from 'globals'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  { ignores: ['dist', 'dist-mock', 'coverage', 'playwright-report', 'test-results', 'src/routeTree.gen.ts', 'scripts-shot.mjs'] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2023,
      globals: { ...globals.browser, ...globals.node },
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true, allowExportNames: ['Route'] }],
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_', destructuredArrayIgnorePattern: '^_' }],
      '@typescript-eslint/consistent-type-imports': ['error', { fixStyle: 'inline-type-imports' }],
      'no-console': ['error', { allow: ['warn', 'error'] }],
    },
  },
  {
    // Route modules, feature modules and fixtures export helpers next to components.
    files: [
      'src/routes/**',
      'src/features/**',
      'src/components/ui/**',
      'src/components/common/**',
      'src/components/brand/**',
      'src/mocks/**',
      'src/test/**',
      'e2e/**',
    ],
    rules: { 'react-refresh/only-export-components': 'off' },
  },
)
