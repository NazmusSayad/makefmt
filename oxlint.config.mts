import { defineConfig } from 'oxlint'
import { recommended, typescriptRules } from './eslint-recommended.mts'

export default defineConfig({
  extends: [recommended],
  plugins: ['typescript', 'import'],
  jsPlugins: ['eslint-plugin-check-file', 'eslint-plugin-oxfmt'],
  options: { typeAware: true },
  ignorePatterns: ['*.config.{js,ts,mjs,mts}'],
  rules: {
    'oxfmt/oxfmt': 'warn',
  },
  overrides: [
    {
      files: ['scripts/**'],
      env: { node: true },
    },
    {
      files: ['**/*.{mts,cts}'],
      rules: typescriptRules,
    },
    {
      files: ['**/*.{ts,tsx}'],
      rules: {
        ...typescriptRules,

        'no-void': 'off',
        'no-empty': 'off',
        'no-useless-escape': 'off',

        'no-eval': 'error',
        'no-param-reassign': 'error',
        'no-empty-function': 'error',
        'no-useless-return': 'error',

        'max-params': ['error', { max: 5, countVoidThis: false }],
        'prefer-arrow-callback': ['error', { allowNamedFunctions: false }],
        'func-style': ['error', 'declaration', { allowArrowFunctions: false }],

        'import/no-default-export': 'error',

        'typescript/no-floating-promises': 'error',
        'no-unused-vars': [
          'warn',
          {
            argsIgnorePattern: '^_',
            caughtErrors: 'all',
            caughtErrorsIgnorePattern: '^_',
            destructuredArrayIgnorePattern: '^_',
            varsIgnorePattern: '^_',
            ignoreRestSiblings: true,
          },
        ],

        'check-file/folder-naming-convention': [
          'error',
          { '*/**': 'KEBAB_CASE' },
        ],
        'check-file/filename-naming-convention': [
          'error',
          { '**/*.*': 'KEBAB_CASE' },
          { ignoreMiddleExtensions: true },
        ],
      },
    },
    {
      files: ['apps/eslint-plugin/src/index.ts'],
      rules: {
        'no-empty-function': 'off',
        'import/no-default-export': 'off',
      },
    },
    {
      files: ['apps/eslint-plugin/test/**'],
      rules: {
        'typescript/no-floating-promises': 'off',
      },
    },
  ],
})
