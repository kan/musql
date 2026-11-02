#!/usr/bin/env node
// 版を tauri.conf.json と Cargo.toml に書き、Cargo.lock の musql の行を合わせる（#149）。
//
//   node scripts/bump-version.mjs X.Y.Z        （just bump X.Y.Z）
//
// ふだんはリリース PR の release-prepare.mjs が呼ぶ。lockfile はビルドせずに更新する
// （`cargo update --workspace` は musql 自身の行だけを変える）。リリース PR を作る ubuntu の
// ランナーでは、`cargo check` は WebView の開発パッケージが無くて通らない。

import { spawnSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fail, root, VERSION_RE } from './release-common.mjs'

const version = process.argv[2]

if (!VERSION_RE.test(version ?? '')) fail('usage: node scripts/bump-version.mjs X.Y.Z')

/** 置き換えが 1 箇所だけに当たることを確かめてから書く（依存の版の行を巻き込まないため）。 */
function bump(relPath, pattern, replacement) {
  const path = join(root, relPath)
  const before = readFileSync(path, 'utf8')
  const hits = [...before.matchAll(pattern)].length
  if (hits !== 1) fail(`${relPath}: 版の行が ${hits} 箇所あります（1 箇所のはず）`)
  const after = before.replace(pattern, replacement)
  if (after !== before) writeFileSync(path, after)
  console.log(`${after !== before ? 'updated  ' : 'unchanged'}  ${relPath}`)
}

// 先頭近くにある musql 自身の version。依存の側に "version" のキーは無い
bump('src-tauri/tauri.conf.json', /^(\s*"version":\s*)"[^"]+"/gm, `$1"${version}"`)
// [package] の version。依存の版は `name = { version = ... }` の形で、行頭が version = にならない
bump('src-tauri/Cargo.toml', /^version = "[^"]+"/gm, `version = "${version}"`)

const r = spawnSync('cargo', ['update', '--workspace', '--quiet'], {
  cwd: join(root, 'src-tauri'),
  stdio: 'inherit',
})
if (r.status !== 0) fail(`cargo update --workspace が失敗しました（終了コード ${r.status ?? r.error}）`)
console.log(`version = ${version}（Cargo.lock も更新した）`)
