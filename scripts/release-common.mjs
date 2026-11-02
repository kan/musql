// リリースのスクリプト（scripts/*.mjs）が共有するもの（#149）: 版の形、失敗の出し方、CHANGELOG.md の
// 節の読み取り。節の読み取りは、release-prepare.mjs（リリース PR）と release-notes.mjs（タグの後の
// draft ジョブ）が同じ規則で読むように、ここに 1 つだけ置く。両者の判定がずれると、リリース PR では
// 通った節がタグを打った後に別の範囲として読まれる。

import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

export const root = resolve(import.meta.dirname, '..')
export const CHANGELOG = join(root, 'CHANGELOG.md')

/** muSQL の版の形（プレリリースは扱わない）。 */
export const VERSION_RE = /^\d+\.\d+\.\d+$/

/** 末尾の比較リンク `[X.Y.Z]: https://...`。 */
export const LINK_RE = /^\[(\d+\.\d+\.\d+)\]: /

export function fail(message) {
  console.error(message)
  process.exit(1)
}

/** CHANGELOG を行に分ける（ファイルは LF。書き戻すときは '\n' で結ぶ）。 */
export function readChangelog() {
  return readFileSync(CHANGELOG, 'utf8').split('\n')
}

/** `## ` の見出しの行番号。コードブロックの中の `## ` は数えない。 */
export function headings(lines) {
  const found = []
  let inFence = false
  for (const [i, line] of lines.entries()) {
    if (/^\s*(```|~~~)/.test(line)) inFence = !inFence
    else if (!inFence && line.startsWith('## ')) found.push(i)
  }
  return found
}

/** 見出し `start` の節の本文（次の見出しか、末尾の比較リンクの手前まで。前後の空白は除く）。 */
export function sectionBody(lines, start) {
  const next = headings(lines).find((i) => i > start) ?? lines.length
  const link = lines.findIndex((line, i) => i > start && LINK_RE.test(line))
  const end = link >= 0 && link < next ? link : next
  return lines
    .slice(start + 1, end)
    .join('\n')
    .trim()
}
