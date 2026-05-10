import "./_setup.ts";

import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const PLUGIN_ROOT = resolve(HERE, "..", "claude-plugin");
const CLIENT_PKG_JSON = resolve(HERE, "..", "package.json");

interface Frontmatter {
  raw: string;
  fields: Record<string, string>;
}

// Minimal YAML frontmatter parser. Plugin docs use a tiny subset (key: value
// or key: "value"); a real YAML lib would be overkill here.
function parseFrontmatter(text: string): Frontmatter | null {
  if (!text.startsWith("---\n")) return null;
  const end = text.indexOf("\n---", 4);
  if (end === -1) return null;
  const raw = text.slice(4, end);
  const fields: Record<string, string> = {};
  for (const line of raw.split("\n")) {
    const m = line.match(/^([A-Za-z_-]+):\s*(.*)$/);
    if (!m) continue;
    let v = m[2].trim();
    if ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'"))) {
      v = v.slice(1, -1);
    }
    fields[m[1]] = v;
  }
  return { raw, fields };
}

describe("claude-plugin: plugin.json", () => {
  const manifestPath = join(PLUGIN_ROOT, ".claude-plugin", "plugin.json");

  test("file exists", () => {
    expect(existsSync(manifestPath)).toBe(true);
  });

  test("has the required minimal fields", () => {
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as {
      name: string;
      version: string;
      description: string;
      commands?: string;
      skills?: string;
    };
    expect(manifest.name).toBe("gtk4-e2e");
    expect(typeof manifest.version).toBe("string");
    expect(manifest.description.length).toBeGreaterThan(0);
    expect(manifest.commands).toBe("./commands/");
    expect(manifest.skills).toBe("./skills/");
  });

  test("plugin version matches packages/client/package.json", () => {
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as {
      version: string;
    };
    const pkg = JSON.parse(readFileSync(CLIENT_PKG_JSON, "utf8")) as {
      version: string;
    };
    expect(manifest.version).toBe(pkg.version);
  });
});

describe("claude-plugin: commands/", () => {
  const commands = ["e2e-tap", "e2e-record", "e2e-scenario"];

  for (const name of commands) {
    test(`commands/${name}.md has frontmatter with description`, () => {
      const path = join(PLUGIN_ROOT, "commands", `${name}.md`);
      expect(existsSync(path)).toBe(true);

      const text = readFileSync(path, "utf8");
      const fm = parseFrontmatter(text);
      expect(fm).not.toBeNull();
      const fields = fm!.fields;
      expect(fields.description).toBeDefined();
      expect(fields.description.length).toBeGreaterThan(0);
    });

    test(`commands/${name}.md uses $ARGUMENTS (not legacy {{ARG}})`, () => {
      const path = join(PLUGIN_ROOT, "commands", `${name}.md`);
      const text = readFileSync(path, "utf8");
      // Body (after frontmatter) shouldn't reference {{ARG}} — Claude Code's
      // current spec is $ARGUMENTS. Don't enforce *presence* of $ARGUMENTS;
      // some commands may be parameterless. Just keep the legacy syntax out.
      expect(text).not.toContain("{{ARG}}");
      expect(text).not.toContain("{{OUTPUT}}");
      expect(text).not.toContain("{{SCENARIO}}");
    });
  }
});

describe("claude-plugin: SKILL.md", () => {
  const skillPath = join(PLUGIN_ROOT, "skills", "gtk4-e2e", "SKILL.md");

  test("file exists at the conventional path", () => {
    expect(existsSync(skillPath)).toBe(true);
  });

  test("frontmatter has name + description", () => {
    const text = readFileSync(skillPath, "utf8");
    const fm = parseFrontmatter(text);
    expect(fm).not.toBeNull();
    const fields = fm!.fields;
    expect(fields.name).toBe("gtk4-e2e");
    expect(fields.description).toBeDefined();
    expect(fields.description.length).toBeGreaterThan(0);
  });

  test("body covers the full CLI surface (info / tap / screenshot / record / scenario)", () => {
    const text = readFileSync(skillPath, "utf8");
    expect(text).toContain("bunx gtk4-e2e info");
    expect(text).toContain("bunx gtk4-e2e tap");
    expect(text).toContain("bunx gtk4-e2e screenshot");
    expect(text).toContain("bunx gtk4-e2e record");
    expect(text).toContain("bun test");
  });
});
