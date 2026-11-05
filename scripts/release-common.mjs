// リリースのスクリプト（scripts/*.mjs）が共有するもの（#149）: リポジトリのルート、版の形、失敗の出し方。
// CHANGELOG.md の読み書きは kan/tagpr-scripts の subtree（`scripts/tagpr/`、#152）が持つ。

import { spawnSync } from 'node:child_process'
import { join, resolve } from 'node:path'

export const root = resolve(import.meta.dirname, '..')

/**
 * ルートからの相対パスの Node のスクリプトを、ルートを作業ディレクトリにして走らせ、終了コードを返す
 * （起動できなければその理由を出して 1）。tagpr-scripts は CHANGELOG.md を作業ディレクトリから読む。
 */
export function runNode(script, args = []) {
  const r = spawnSync(process.execPath, [join(root, script), ...args], { cwd: root, stdio: 'inherit' })
  if (r.error) console.error(`${script} を起動できませんでした: ${r.error.message}`)
  return r.status ?? 1
}

/** muSQL の版の形（プレリリースは扱わない）。 */
export const VERSION_RE = /^\d+\.\d+\.\d+$/

export function fail(message) {
  console.error(message)
  process.exit(1)
}
