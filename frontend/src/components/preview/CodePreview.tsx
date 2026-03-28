/**
 * @file Code Preview
 *
 * Renders source code with syntax highlighting (via Shiki) and line numbers.
 * Shiki is loaded lazily on first use — the highlighter instance is cached
 * at module level so subsequent previews reuse it without re-downloading
 * grammars.
 *
 * The highlighting theme is "ayu-dark" which closely matches DiskDeck's
 * Ayu color scheme. The output HTML is rendered via `innerHTML` — this is
 * safe because Shiki generates static `<span>` elements with inline styles,
 * no executable content.
 *
 * For files without a known language grammar, falls back to plain text
 * with line numbers (no highlighting).
 *
 * @module components/preview/CodePreview
 */

import { createSignal, onMount, type Component } from "solid-js";
import { getLang } from "../../lib/languages";
import styles from "./CodePreview.module.css";

/** Lazily-initialized Shiki highlighter (cached at module level). */
let highlighterPromise: Promise<import("shiki").Highlighter> | null = null;

/**
 * Returns a cached Shiki highlighter instance, creating it on first call.
 * Uses dynamic import so Shiki's WASM + grammars are only loaded when
 * a code preview is actually opened.
 */
async function getHighlighter() {
  if (!highlighterPromise) {
    highlighterPromise = import("shiki").then((shiki) =>
      shiki.createHighlighter({
        themes: ["ayu-dark"],
        langs: [],
      }),
    );
  }
  return highlighterPromise;
}

/**
 * Code preview component with syntax highlighting and line numbers.
 *
 * @param props.content  - The raw file content as a string.
 * @param props.fileName - Used to determine the language for highlighting.
 */
export const CodePreview: Component<{
  content: string;
  fileName: string;
}> = (props) => {
  const [html, setHtml] = createSignal<string | null>(null);
  const lang = getLang(props.fileName);

  onMount(async () => {
    if (!lang) {
      // No grammar — render as plain text with line numbers
      setHtml(escapeAndWrap(props.content));
      return;
    }

    try {
      const highlighter = await getHighlighter();

      // Lazily load the language grammar if not already loaded
      const loaded = highlighter.getLoadedLanguages();
      if (!loaded.includes(lang)) {
        await highlighter.loadLanguage(lang as Parameters<typeof highlighter.loadLanguage>[0]);
      }

      const result = highlighter.codeToHtml(props.content, {
        lang,
        theme: "ayu-dark",
      });
      setHtml(result);
    } catch {
      // Fallback: plain text if highlighting fails
      setHtml(escapeAndWrap(props.content));
    }
  });

  return (
    <div class={styles.wrapper}>
      {html()
        ? <div class={styles.code} innerHTML={html()!} />
        : <div class={styles.loading}>Highlighting...</div>
      }
    </div>
  );
};

/**
 * Escapes HTML entities and wraps content in a `<pre><code>` block
 * matching Shiki's output structure, used as a fallback when no
 * grammar is available.
 */
function escapeAndWrap(content: string): string {
  const escaped = content
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
  return `<pre class="shiki" style="background-color:transparent"><code>${escaped}</code></pre>`;
}
