// Shared ESLint flat config. Packages run `eslint .` from their own folder.
import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import react from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      "**/dist/**",
      "**/.next/**",
      "apps/web/out/**",
      "apps/web/.source/**",
      "**/.vercel/**",
      "apps/web/public/demo/app/**",
      "**/node_modules/**",
      "**/target/**",
      "**/src-tauri/gen/**",
      "**/next-env.d.ts",
      "**/*.config.{js,mjs,cjs}",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: { ...globals.browser, ...globals.node },
    },
    rules: {
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
    },
  },
  {
    // Scripts that run in web pages: the element picker (F-CD-18) and the
    // site's demo loader (D-128).
    files: ["apps/desktop/src-tauri/src/**/*.js", "src-tauri/src/**/*.js", "apps/web/public/**/*.js", "public/**/*.js"],
    languageOptions: { ecmaVersion: 2022, sourceType: "script", globals: globals.browser },
  },
  {
    files: ["**/*.tsx"],
    plugins: { react, "react-hooks": reactHooks },
    settings: { react: { version: "19" } },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react/jsx-key": "error",
    },
  },
  {
    // User-visible strings live in the i18n catalog (PRODUCT_SPEC §0.3).
    files: ["apps/desktop/src/**/*.tsx", "src/**/*.tsx"],
    ignores: ["**/*.test.tsx", "**/test/**"],
    plugins: { react },
    rules: {
      "react/jsx-no-literals": [
        "error",
        { noStrings: true, ignoreProps: true, allowedStrings: ["·", "…", "—", "/", "%", "+", "×", "‹", "›"] },
      ],
    },
  },
  prettier,
);
