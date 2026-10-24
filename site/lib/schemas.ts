/**
 * Committed versioned JSON Schemas for `grit --json` outcomes (`content/schemas/<command>.v<N>.json`).
 */
import fs from "node:fs";
import path from "node:path";
import type { Site } from "./docs";
import { docsContentDir, readRepoFile, REPO_ROOT, SITE_URL } from "./repo";

const SCHEMA_FILE = /^(.+)\.v(\d+)\.json$/;

/** Public URL for a command path schema document (`status`, `stash.list`, …). */
export function schemaPublicUrl(command: string): string {
  return `${SITE_URL}/schemas/${command}/v1.json`;
}

/** Public URL for a committed schema file. */
export function schemaPublicUrlFor(schema: SchemaFile): string {
  return `${SITE_URL}/schemas/${schema.command}/v${schema.version}.json`;
}

/** Site-relative href for a schema (for Markdown in docs pages). */
export function schemaHref(command: string): string {
  return `/schemas/${command}/v1.json`;
}

/** Site-relative href for a v2 schema document. */
export function schemaHrefV2(command: string): string {
  return schemaHrefFor(command, 2);
}

/** Site-relative href for a schema document of any version. */
export function schemaHrefFor(command: string, version: number): string {
  return `/schemas/${command}/v${version}.json`;
}

export function schemasContentDir(): string {
  return process.env.GRIT_SCHEMAS_CONTENT ?? path.join(REPO_ROOT, "content", "schemas");
}

export interface SchemaFile {
  command: string;
  version: number;
  relPath: string;
  body: string;
}

let cache: SchemaFile[] | undefined;

function loadSchemaFiles(): SchemaFile[] {
  return fs
    .readdirSync(schemasContentDir())
    .filter((n) => n.endsWith(".json"))
    .sort()
    .map((name) => {
      const m = SCHEMA_FILE.exec(name);
      if (!m) throw new Error(`unexpected schema file name ${name}`);
      const relPath = path.join("content", "schemas", name);
      return { command: m[1], version: Number(m[2]), relPath, body: readRepoFile(relPath) };
    })
    .sort((a, b) => a.command.localeCompare(b.command) || a.version - b.version);
}

/** Every committed schema file, sorted by command path then version. */
export function loadSchemas(): SchemaFile[] {
  if (cache) return cache;
  if (!fs.existsSync(schemasContentDir())) {
    throw new Error(`missing schemas directory ${schemasContentDir()}`);
  }
  cache = loadSchemaFiles();
  return cache;
}

/** Lookup one schema by command path and version. */
export function findSchema(command: string, version: number): SchemaFile | undefined {
  return loadSchemas().find((s) => s.command === command && s.version === version);
}

export function resetSchemasCache(): void {
  cache = undefined;
}

/** Docs page slug that should link a schema (command pages and `agents` for `error`). */
export function schemaDocSlug(command: string): string {
  if (command === "error") return "agents";
  return command.split(".")[0];
}

const SCHEMA_LINK = /\/schemas\/([a-z0-9_.-]+)\/v(\d+)\.json/g;

function markdownSources(): string[] {
  const contentDir = docsContentDir();
  const paths = [path.join(contentDir, "agents.md"), path.join(contentDir, "schemas.md")];
  const commands = path.join(contentDir, "commands");
  if (fs.existsSync(commands)) {
    for (const name of fs.readdirSync(commands)) {
      if (name.endsWith(".md") && name !== "README.md") paths.push(path.join(commands, name));
    }
  }
  return paths.filter((p) => fs.existsSync(p));
}

/** Collect schema hrefs (`/schemas/<command>/v<N>.json`) linked from docs Markdown sources. */
export function linkedSchemaHrefs(_site: Site): Set<string> {
  const linked = new Set<string>();
  for (const file of markdownSources()) {
    const text = fs.readFileSync(file, "utf8");
    for (const m of text.matchAll(SCHEMA_LINK)) linked.add(schemaHrefFor(m[1], Number(m[2])));
  }
  return linked;
}

/**
 * Every schema must be linked from a docs page; every `/schemas/.../v<N>.json` link
 * must match a committed schema file.
 */
export function validateSchemaLinks(site: Site): string[] {
  const onDisk = new Map(loadSchemas().map((s) => [schemaHrefFor(s.command, s.version), s]));
  const linked = linkedSchemaHrefs(site);
  const problems: string[] = [];
  for (const [href, schema] of onDisk) {
    if (!linked.has(href)) {
      problems.push(
        `${schema.relPath}: no docs page links ${href} (expected on /docs/${schemaDocSlug(schema.command)}/)`,
      );
    }
  }
  for (const href of linked) {
    if (!onDisk.has(href)) problems.push(`docs link ${href} but no matching file in content/schemas`);
  }
  return problems.sort();
}

/** Markdown line for a single schema link (used in command pages). */
export function schemaLinkLine(command: string): string {
  return `Schema: [v1](${schemaHref(command)})`;
}
