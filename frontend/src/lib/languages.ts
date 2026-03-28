/**
 * @file Language Mapping
 *
 * Maps file extensions to Shiki language IDs for syntax highlighting.
 * Only extensions with a known Shiki grammar are listed; unlisted
 * extensions fall back to plain text rendering.
 *
 * @module lib/languages
 */

/** Map of file extension (lowercase, no dot) to Shiki language ID. */
const extToLang: Record<string, string> = {
  // Web
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "jsx",
  ts: "typescript",
  tsx: "tsx",
  html: "html",
  htm: "html",
  css: "css",
  scss: "scss",
  less: "less",
  vue: "vue",
  svelte: "svelte",
  astro: "astro",
  // Data / Config
  json: "json",
  jsonc: "jsonc",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  xml: "xml",
  csv: "csv",
  ini: "ini",
  env: "shellscript",
  // Systems
  rs: "rust",
  go: "go",
  c: "c",
  h: "c",
  cpp: "cpp",
  hpp: "cpp",
  java: "java",
  kt: "kotlin",
  swift: "swift",
  cs: "csharp",
  // Scripting
  py: "python",
  rb: "ruby",
  php: "php",
  lua: "lua",
  pl: "perl",
  r: "r",
  // Shell
  sh: "shellscript",
  bash: "shellscript",
  zsh: "shellscript",
  fish: "shellscript",
  ps1: "powershell",
  bat: "bat",
  cmd: "bat",
  // Markup / Docs
  md: "markdown",
  mdx: "mdx",
  tex: "latex",
  // DevOps / Config
  dockerfile: "dockerfile",
  docker: "dockerfile",
  tf: "hcl",
  hcl: "hcl",
  nix: "nix",
  // Database
  sql: "sql",
  graphql: "graphql",
  gql: "graphql",
  prisma: "prisma",
  // Misc
  makefile: "makefile",
  cmake: "cmake",
  diff: "diff",
  patch: "diff",
  log: "log",
  gitignore: "shellscript",
  lock: "json",
};

/**
 * Resolves a filename to a Shiki language ID.
 * Checks the extension first, then tries the full filename (for
 * extensionless files like "Dockerfile", "Makefile").
 *
 * @returns The Shiki language ID, or null if no grammar is known.
 */
export function getLang(fileName: string): string | null {
  const ext = fileName.split(".").pop()?.toLowerCase() ?? "";
  if (extToLang[ext]) return extToLang[ext];

  // Check full filename for extensionless convention files
  const lower = fileName.toLowerCase();
  if (extToLang[lower]) return extToLang[lower];

  return null;
}
