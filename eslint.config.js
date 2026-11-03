import stylistic from '@stylistic/eslint-plugin';
import pluginVue from 'eslint-plugin-vue';
import globals from 'globals';
import tseslint from 'typescript-eslint';
import vueParser from 'vue-eslint-parser';
import singleLineStartTag from './eslint/rules/single-line-start-tag.js';

export default [
    {
        ignores: [
            'dist/**',
            'node_modules/**',
            'coverage/**',
            'src-tauri/target/**',
            'src-tauri/gen/**',
            'src/types/bindings.ts',
            '.worktrees/**',
            '.max-code-worktrees/**',
            'tests/lint/fixtures/invalid-*.vue',
            'tests/lint/fixtures/invalid-*.ts'
        ]
    },
    ...tseslint.configs.recommended,
    ...pluginVue.configs['flat/recommended'],
    {
        files: ['**/*.{js,mjs,ts,vue}'],
        plugins: {
            '@stylistic': stylistic,
            max: {
                rules: {
                    'single-line-start-tag': singleLineStartTag
                }
            }
        },
        languageOptions: {
            ecmaVersion: 'latest',
            sourceType: 'module',
            globals: {
                ...globals.browser,
                ...globals.node
            }
        },
        rules: {
            '@stylistic/indent': ['error', 4, { SwitchCase: 1 }],
            '@stylistic/quotes': ['error', 'single', { avoidEscape: true }],
            '@stylistic/semi': ['error', 'always'],
            '@stylistic/comma-dangle': ['error', 'never'],
            '@typescript-eslint/no-unused-vars': [
                'error',
                {
                    argsIgnorePattern: '^_',
                    varsIgnorePattern: '^_'
                }
            ],
            '@typescript-eslint/consistent-type-imports': 'error',
            '@typescript-eslint/no-explicit-any': 'error'
        }
    },
    {
        files: ['**/*.vue'],
        languageOptions: {
            parser: vueParser,
            parserOptions: {
                parser: tseslint.parser,
                extraFileExtensions: ['.vue'],
                sourceType: 'module'
            }
        },
        rules: {
            '@stylistic/indent': 'off',
            'vue/block-order': [
                'error',
                {
                    order: ['template', 'script[setup]', 'style[scoped]']
                }
            ],
            'vue/component-api-style': ['error', ['script-setup']],
            'vue/block-lang': [
                'error',
                {
                    script: { lang: 'ts' },
                    style: { lang: 'scss' }
                }
            ],
            'vue/enforce-style-attribute': ['error', { allow: ['scoped'] }],
            'vue/html-indent': ['error', 4],
            'vue/script-indent': [
                'error',
                4,
                {
                    baseIndent: 1,
                    switchCase: 1
                }
            ],
            'vue/html-quotes': ['error', 'double'],
            'vue/max-attributes-per-line': 'off',
            'vue/singleline-html-element-content-newline': 'off',
            'vue/multiline-html-element-content-newline': 'off',
            'vue/first-attribute-linebreak': [
                'error',
                {
                    singleline: 'beside',
                    multiline: 'beside'
                }
            ],
            'vue/html-closing-bracket-newline': [
                'error',
                {
                    singleline: 'never',
                    multiline: 'never'
                }
            ],
            'vue/define-props-declaration': ['error', 'type-based'],
            'vue/define-emits-declaration': ['error', 'type-literal'],
            'vue/no-v-html': 'error',
            'max/single-line-start-tag': 'error'
        }
    }
];
