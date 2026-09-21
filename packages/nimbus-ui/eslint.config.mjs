// Design-system lint for the operator console. Biome owns general lint and
// formatting (`biome check src`); this file owns only the @shadcn/lint rules,
// which read components.json, src/styles/globals.css and the cva variants in
// src/components/ui to verify that call sites style through tokens and
// variants. DESIGN.md "Styling Contract" is the prose these rules enforce.
//
// Adoption follows shadcn-ui/lint docs/adoption.md: the two rules that catch
// dead CSS run as errors; the rest run as warnings behind the --max-warnings
// cap in package.json. Lower the cap as violations are fixed; never raise it.
import { plugin as shadcn } from "@shadcn/lint";
import tsParser from "@typescript-eslint/parser";
import { defineConfig, globalIgnores } from "eslint/config";

// Data-bearing controls and titles may take the mono voice and the compact
// steps of the type scale (DESIGN.md: "Mono is the voice of data"). Every
// other class category on a registry component is a variant's job.
const DATA_TYPOGRAPHY = [
  "layout",
  "font-mono",
  "tabular",
  "truncate",
  "text-xs",
  "text-sm",
];

export default defineConfig([
  globalIgnores([
    "dist/**",
    "storybook-static/**",
    "node_modules/**",
    "src/route-tree.gen.ts",
    "convex/_generated/**",
    ".nimbus/**",
  ]),
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parser: tsParser,
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    plugins: { shadcn },
    settings: {
      shadcn: {
        // Nimbus-owned components under src/components are design-system
        // components too: a call site restyles them only through props.
        componentImports: ["^@/components(/|$)"],
        note: "See DESIGN.md → Implementation Rules → Styling Contract for the rules and the approved exceptions.",
      },
    },
    rules: {
      // A class Tailwind cannot generate is dead CSS: no colour, no size.
      "shadcn/no-raw-colors": "error",
      "shadcn/no-unknown-classes": "error",
      // Warnings, capped in package.json. Promote to "error" as they reach 0.
      "shadcn/no-restyle": [
        "warn",
        {
          allow: ["layout"],
          contracts: [
            {
              pattern:
                "^(Input|Textarea|InputGroupInput|InputGroupTextarea|SelectTrigger|SelectValue|SelectItem|CommandInput|CommandItem|SheetTitle|DialogTitle|Kbd)$",
              allow: DATA_TYPOGRAPHY,
            },
          ],
        },
      ],
      "shadcn/no-arbitrary-values": ["warn", { allow: ["layout"] }],
      "shadcn/no-inline-styles": "warn",
      "shadcn/require-static-classes": "warn",
    },
  },
  {
    // Registry files are CLI-owned (`npx shadcn@latest add|diff`) and style
    // themselves. Keep them as shipped so `shadcn diff` stays clean.
    files: ["src/components/ui/**"],
    rules: {
      "shadcn/no-restyle": "off",
      "shadcn/no-arbitrary-values": "off",
      "shadcn/require-static-classes": "off",
      // `toaster` is sonner's own hook class, shipped by the registry.
      "shadcn/no-unknown-classes": ["error", { allow: ["toaster"] }],
    },
  },
  {
    // The cn() spec passes placeholder class names on purpose.
    files: ["src/lib/utils.spec.ts"],
    rules: { "shadcn/no-unknown-classes": "off" },
  },
]);
