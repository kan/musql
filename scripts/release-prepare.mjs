#!/usr/bin/env node
// リリース PR の中身をそろえる（tagpr の postVersionCommand、#149）。
//
//   TAGPR_CURRENT_VERSION=vX.Y.Z TAGPR_NEXT_VERSION=vX.Y.Z node scripts/release-prepare.mjs
//
// tagpr は .tagpr の versionFile（tauri.conf.json）だけを書き換えてからこれを呼ぶ。main が
// 進むたびにリリース PR のブランチを main から作り直して呼び直す（書き換え済みのツリーで
// 走らせ直すと、CHANGELOG の段が「タグより新しい版の節がある」で落ちる）。
//
// 1. CHANGELOG の `## [Unreleased]` を `## [X.Y.Z] - 日付` にし、空の `## [Unreleased]` を
//    上に戻して、末尾にその版のリンクを足す（kan/tagpr-scripts の subtree、#152。
//    `scripts/tagpr/` の中は編集しない。直すときは kan/tagpr-scripts を直して subtree pull する）
// 2. Cargo.toml の版と Cargo.lock を合わせる（bump-version.mjs）
// 3. ライセンス一覧（ui/credits.json）を作り直す（credits.mjs）
//
// CHANGELOG の段が落ちたら（リリースの直後は `## [Unreleased]` が空なので毎回落ちる）、残りは
// 走らせずに失敗で終える。tagpr は失敗してもリリース PR を作るので、Cargo.toml などまで
// 書き換えた PR を残さない。2 と 3 は、片方が落ちても残りを走らせ、最後に失敗で終える。
// run が赤ならマージしない。

import { fail, runNode, VERSION_RE } from './release-common.mjs'

const version = (process.env.TAGPR_NEXT_VERSION ?? '').replace(/^v/, '')

if (!VERSION_RE.test(version)) {
  fail(`TAGPR_NEXT_VERSION が vX.Y.Z の形ではありません: "${process.env.TAGPR_NEXT_VERSION ?? ''}"`)
}

const failures = []

function runScript(label, script, args = []) {
  console.log(`\n== ${label}`)
  const status = runNode(script, args)
  if (status !== 0) failures.push(`${label}（終了コード ${status}）`)
  return status === 0
}

// TAGPR_NEXT_VERSION と TAGPR_CURRENT_VERSION は環境変数のまま渡る。落ちるときは何も書き換えない
if (runScript('CHANGELOG', 'scripts/tagpr/release-prepare.mjs', ['--changelog-only'])) {
  runScript('bump', 'scripts/bump-version.mjs', [version])
  runScript('credits', 'scripts/credits.mjs')
}

if (failures.length > 0) {
  console.error(`\n失敗したステップ:\n${failures.map((f) => `- ${f}`).join('\n')}`)
  process.exit(1)
}
