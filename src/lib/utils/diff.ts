/**
 * Block-level text diff for version history.
 *
 * Splits documents into blocks (paragraphs) and uses a simple LCS-based diff
 * to classify each block as added, removed, changed, or unchanged.
 */

import type { DiffBlock } from '../types/index.js';

/** Split text into blocks (paragraphs separated by blank lines or single newlines). */
function splitBlocks(text: string): string[] {
  // Split on double newlines for paragraph boundaries
  // Single newlines within a paragraph are kept together
  return text
    .split(/\n{2,}/)
    .map((block) => block.trim())
    .filter((block) => block.length > 0);
}

/**
 * Compute a block-level diff between old and new text.
 * Returns an array of DiffBlock entries (only non-unchanged blocks).
 */
export function computeBlockDiff(oldText: string, newText: string): DiffBlock[] {
  const oldBlocks = splitBlocks(oldText);
  const newBlocks = splitBlocks(newText);

  // Use Myers-like diff via LCS on block fingerprints
  const lcs = longestCommonSubsequence(oldBlocks, newBlocks);
  const diffs: DiffBlock[] = [];

  let oldIdx = 0;
  let newIdx = 0;
  let lineCounter = 1;

  for (const [oi, ni] of lcs) {
    // Blocks before this match in old = removed
    while (oldIdx < oi) {
      diffs.push({
        type: 'removed',
        content: oldBlocks[oldIdx]!,
        lineStart: lineCounter,
        lineEnd: lineCounter,
      });
      oldIdx++;
    }

    // Blocks before this match in new = added
    while (newIdx < ni) {
      diffs.push({
        type: 'added',
        content: newBlocks[newIdx]!,
        lineStart: lineCounter,
        lineEnd: lineCounter,
      });
      lineCounter++;
      newIdx++;
    }

    // Matched block — check if content is exactly equal or just similar
    if (oldBlocks[oi] === newBlocks[ni]) {
      // Unchanged — include for context
      diffs.push({
        type: 'unchanged',
        content: newBlocks[ni]!,
        lineStart: lineCounter,
        lineEnd: lineCounter,
      });
      lineCounter++;
    } else {
      // Changed (same position, different content)
      diffs.push({
        type: 'changed',
        content: newBlocks[ni]!,
        lineStart: lineCounter,
        lineEnd: lineCounter,
      });
      lineCounter++;
    }

    oldIdx = oi + 1;
    newIdx = ni + 1;
  }

  // Remaining blocks in old = removed
  while (oldIdx < oldBlocks.length) {
    diffs.push({
      type: 'removed',
      content: oldBlocks[oldIdx]!,
      lineStart: lineCounter,
      lineEnd: lineCounter,
    });
    oldIdx++;
  }

  // Remaining blocks in new = added
  while (newIdx < newBlocks.length) {
    diffs.push({
      type: 'added',
      content: newBlocks[newIdx]!,
      lineStart: lineCounter,
      lineEnd: lineCounter,
    });
    lineCounter++;
    newIdx++;
  }

  return diffs;
}

/**
 * Compute LCS (Longest Common Subsequence) indices.
 * Returns pairs of [oldIndex, newIndex] for matching blocks.
 */
function longestCommonSubsequence(a: string[], b: string[]): Array<[number, number]> {
  const m = a.length;
  const n = b.length;

  // DP table
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));

  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      if (a[i - 1] === b[j - 1]) {
        dp[i]![j] = dp[i - 1]![j - 1]! + 1;
      } else {
        dp[i]![j] = Math.max(dp[i - 1]![j]!, dp[i]![j - 1]!);
      }
    }
  }

  // Backtrack to find the actual LCS indices
  const result: Array<[number, number]> = [];
  let i = m;
  let j = n;

  while (i > 0 && j > 0) {
    if (a[i - 1] === b[j - 1]) {
      result.unshift([i - 1, j - 1]);
      i--;
      j--;
    } else if (dp[i - 1]![j]! > dp[i]![j - 1]!) {
      i--;
    } else {
      j--;
    }
  }

  return result;
}
