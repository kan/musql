#!/usr/bin/env node
// リリース PR の中身をそろえる（tagpr の postVersionCommand、#149）。
//
//   TAGPR_CURRENT_VERSION=vX.Y.Z TAGPR_NEXT_VERSION=vX.Y.Z TAGPR_ARTIFACT_DIR=<dir> \
//     node scripts/release-prepare.mjs
//
// tagpr は .tagpr の versionFile（tauri.conf.json と Cargo.toml）の版を書き換えてからこれを呼ぶ。
// main が進むたびにリリース PR のブランチを main から作り直して呼び直す（書き換え済みのツリーで
// 走らせ直すと、CHANGELOG の段が「タグより新しい版の節がある」で落ちる）。
//
// 1. CHANGELOG の `## [Unreleased]` を `## [X.Y.Z] - 日付` にし、空の `## [Unreleased]` を
//    上に戻して、末尾にその版のリンクを足す。.tagpr の versionFile のすべてのファイルの版が
//    提案と同じかを確かめ、Cargo.lock の musql の行を合わせる（kan/tagpr-scripts の subtree、#152 / #154。
//    `scripts/tagpr/` の中は編集しない。直すときは kan/tagpr-scripts を直して subtree pull する）
// 2. ライセンス一覧（ui/credits.json）を TAGPR_ARTIFACT_DIR/credits.json で置き換える。
//    **ここでは作らない。** 作るには `cargo metadata`（依存のソースの取得）と SPDX の標準文の取得が
//    要るので、tagpr.yml の書き込みの権限を持たない credits ジョブが main のツリーで作り、artifact
//    `credits` で渡す。kan/tagpr-scripts の共通の workflow がそれを展開し、そのディレクトリを
//    TAGPR_ARTIFACT_DIR で渡す（kan/tagpr-scripts#6。展開に失敗すると空になる）。一覧は musql 自身を
//    含まないので、版を上げる前のツリーで作っても同じものになる。
//    手元で試すときは、先に `just credits` を走らせて TAGPR_ARTIFACT_DIR=ui を渡す
//
// TAGPR_ARTIFACT_DIR に credits.json が無いか、1 が落ちたら（`## [Unreleased]` が空なときなど。
// リリースの直後の push の run では、共通の workflow が tagpr を走らせないのでここまで来ない）、CHANGELOG も Cargo.lock も ui/credits.json も書き換えずに失敗で終える
// （credits.json の有無は 1 より前に見る。1 は検査をすべて済ませてから書く）。ただし
// tauri.conf.json と Cargo.toml は tagpr がこれを呼ぶ前に上げている。tagpr は失敗してもその状態で
// リリース PR を作るので、共通の workflow がそれを draft にする。tagpr の run が赤ならマージしない。

import { copyFileSync, existsSync, realpathSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fail, root, runNode } from './release-common.mjs'

const CREDITS = join(root, 'ui', 'credits.json')

// 相対パスはリポジトリのルートから読む（tagpr もルートで呼ぶ）
const artifactDir = process.env.TAGPR_ARTIFACT_DIR ?? ''
const credits = artifactDir ? resolve(root, artifactDir, 'credits.json') : ''
if (!credits || !existsSync(credits) || !statSync(credits).isFile()) {
  fail(`TAGPR_ARTIFACT_DIR に作り直したライセンス一覧（credits.json）のあるディレクトリを渡してください: "${artifactDir}"`)
}

// TAGPR_NEXT_VERSION と TAGPR_CURRENT_VERSION は環境変数のまま渡る（形の検査も向こうが行う）
console.log('\n== CHANGELOG と Cargo.lock')
const status = runNode('scripts/tagpr/release-prepare.mjs', ['--changelog-only', '--cargo', 'src-tauri'])
if (status !== 0) fail(`\n失敗したステップ: CHANGELOG と Cargo.lock（終了コード ${status}）`)

console.log('\n== credits')
// 同じファイルかは実体のパスで比べる（Windows ではドライブ文字や大文字小文字の違いで文字列が食い違う）
if (realpathSync.native(credits) === realpathSync.native(CREDITS)) console.log('ui/credits.json をそのまま使う')
else {
  copyFileSync(credits, CREDITS)
  console.log(`${credits} -> ui/credits.json`)
}
