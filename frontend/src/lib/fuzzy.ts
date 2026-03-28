/**
 * @file Fuzzy Matching Utilities
 *
 * Provides simple fuzzy matching functions for the command palette.
 * Used to filter and rank results as the user types a query.
 *
 * @module lib/fuzzy
 */

/**
 * Check if a query is a case-insensitive substring of the given text.
 *
 * @param query - The search string entered by the user.
 * @param text  - The candidate text to match against.
 * @returns True if `query` appears anywhere in `text` (case-insensitive).
 */
export function fuzzyMatch(query: string, text: string): boolean {
  return text.toLowerCase().includes(query.toLowerCase());
}

/**
 * Score a fuzzy match by the position of the first occurrence.
 * Lower scores indicate better matches: an exact prefix match scores 0,
 * a mid-string match scores the index where the match starts, and
 * a non-match scores the full text length (worst possible).
 *
 * @param query - The search string entered by the user.
 * @param text  - The candidate text to score against.
 * @returns A non-negative integer score (lower = better match).
 */
export function fuzzyScore(query: string, text: string): number {
  const index = text.toLowerCase().indexOf(query.toLowerCase());
  return index === -1 ? text.length : index;
}
