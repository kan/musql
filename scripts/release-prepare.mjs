#!/usr/bin/env node
// リリース PR の中身をそろえる（tagpr の postVersionCommand、#149）。
//
//   TAGPR_CURRENT_VERSION=vX.Y.Z TAGPR_NEXT_VERSION=vX.Y.Z node scripts/release-prepare.mjs
//
// tagpr は .tagpr の versionFile（tauri.conf.json と Cargo.toml）の版を書き換えてからこれを呼ぶ。
// main が進むたびにリリース PR のブランチを main から作り直して呼び直す（書き換え済みのツリーで
// 走らせ直すと、CHANGELOG の段が「タグより新しい版の節がある」で落ちる）。
//
// 1. CHANGELOG の `## [Unreleased]` を `## [X.Y.Z] - 日付` にし、空の `## [Unreleased]` を
//    上に戻して、末尾にその版のリンクを足す。.tagpr の versionFile のすべてのファイルの版が
//    提案と同じかを確かめ、Cargo.lock の musql の行を合わせる（kan/tagpr-scripts の subtree、#152 / #154。
//    `scripts/tagpr/` の中は編集しない。直すときは kan/tagpr-scripts を直して subtree pull する）
// 2. ライセンス一覧（ui/credits.json）を作り直す（credits.mjs）
//
// 1 が落ちたら（リリースの直後は `## [Unreleased]` が空なので毎回落ちる）、2 は走らせずに
// 失敗で終える。1 は検査をすべて済ませてから書くので、落ちたときは CHANGELOG も Cargo.lock も
// 書き換えていない。ただし tauri.conf.json と Cargo.toml は tagpr がこれを呼ぶ前に上げている。
// tagpr は失敗してもその状態でリリース PR を作るので、run が赤ならマージしない。

import { runNode } from './release-common.mjs'

function runScript(label, script, args = []) {
  console.log(`\n== ${label}`)
  const status = runNode(script, args)
  if (status !== 0) {
    console.error(`\n失敗したステップ: ${label}（終了コード ${status}）`)
    process.exit(1)
  }
}

// TAGPR_NEXT_VERSION と TAGPR_CURRENT_VERSION は環境変数のまま渡る（形の検査も向こうが行う）
runScript('CHANGELOG と Cargo.lock', 'scripts/tagpr/release-prepare.mjs', ['--changelog-only', '--cargo', 'src-tauri'])
runScript('credits', 'scripts/credits.mjs')
