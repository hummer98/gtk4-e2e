// Test setup helper. The leading underscore is intentional — Bun's test runner
// only picks up `*.test.ts`, so `_setup.ts` will not be executed as a test.
// Each test file imports this module once at the top to ensure
// `src/types.gen.ts` exists before TypeScript resolves the import graph.
//
// CI generates `types.gen.ts` ahead of time via
// `bun packages/client/scripts/gen-types.ts`; this fallback covers fresh
// developer checkouts where the generated file is absent (it is gitignored).

import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const generated = join(here, "..", "src", "types.gen.ts");

if (!existsSync(generated)) {
  const { generate } = await import("../scripts/gen-types.ts");
  await generate();
}
