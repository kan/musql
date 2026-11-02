#!/usr/bin/env node
// リリース PR の中身をそろえる（tagpr の postVersionCommand、#149）。
//
//   TAGPR_NEXT_VERSION=vX.Y.Z node scripts/release-prepare.mjs
//
// tagpr は .tagpr の versionFile（tauri.conf.json）だけを書き換えてからこれを呼ぶ。main が
// 進むたびにリリース PR のブランチを作り直して呼び直すので、何度走っても同じ結果にする。
//
// 1. Cargo.toml の版と Cargo.lock を合わせる（bump-version.mjs）
// 2. CHANGELOG の `## [Unreleased]` を `## [X.Y.Z] - 日付` にし、末尾に比較リンクを足す。
//    節の中身は各 PR が書く
// 3. ライセンス一覧（ui/credits.json）を作り直す（credits.mjs）
//
// 1 つが落ちても残りは走らせ、最後に失敗で終える。tagpr は失敗してもリリース PR を作るので、
// run が赤ならマージしない。

import { spawnSync } from 'node:child_process'
import { writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { CHANGELOG, fail, headings, LINK_RE, readChangelog, root, sectionBody, VERSION_RE } from './release-common.mjs'

const version = (process.env.TAGPR_NEXT_VERSION ?? '').replace(/^v/, '')
const REPO_URL = 'https://github.com/kan/musql'

if (!VERSION_RE.test(version)) {
  fail(`TAGPR_NEXT_VERSION が vX.Y.Z の形ではありません: "${process.env.TAGPR_NEXT_VERSION ?? ''}"`)
}

const failures = []

function runScript(label, script, args = []) {
  console.log(`\n== ${label}`)
  const r = spawnSync(process.execPath, [join(root, script), ...args], { cwd: root, stdio: 'inherit' })
  if (r.status !== 0) failures.push(`${label}（終了コード ${r.status ?? r.error}）`)
}

/** 見出しを版と日付にし、比較リンクを足す。書き換えのどちらも、済んでいれば何もしない。 */
function stampChangelog() {
  console.log('\n== CHANGELOG')
  const lines = readChangelog()
  const found = headings(lines)
  const unreleased = found.filter((i) => /^## \[Unreleased\]\s*$/i.test(lines[i]))
  const stamped = found.filter((i) => lines[i].startsWith(`## [${version}]`))
  // 同じブランチで呼び直されたときは、見出しが書き換え済みの節が 1 つある
  if (unreleased.length + stamped.length !== 1) {
    failures.push(
      `CHANGELOG（## [Unreleased] が ${unreleased.length} 個、## [${version}] が ${stamped.length} 個ある。` +
        '## [Unreleased] が 1 個だけあるはず。利用者に届く変更の PR がこの節に項目を足す）',
    )
    return
  }
  const section = unreleased[0] ?? stamped[0]
  // 空のままマージすると、タグを打った後の draft ジョブ（release-notes.mjs）で初めて落ちる
  if (!sectionBody(lines, section)) {
    failures.push('CHANGELOG（## [Unreleased] の節が空。利用者に届く変更の PR がこの節に項目を足す）')
    return
  }
  if (unreleased.length === 1) {
    // 日付は日本時間で付ける（ランナーは UTC）
    const date = new Intl.DateTimeFormat('sv-SE', { timeZone: 'Asia/Tokyo' }).format(new Date())
    lines[section] = `## [${version}] - ${date}`
    console.log(`## [Unreleased] -> ## [${version}] - ${date}`)
  }

  // 末尾の比較リンク。先頭の行（いちばん新しい版）の版が、前回の版になる
  const links = lines.flatMap((line, i) => {
    const m = LINK_RE.exec(line)
    return m ? [{ i, version: m[1] }] : []
  })
  if (links.length === 0) {
    failures.push('CHANGELOG（末尾に比較リンク `[X.Y.Z]: ...` が 1 つも無い）')
    return
  }
  if (!links.some((l) => l.version === version)) {
    const previous = links[0].version
    lines.splice(links[0].i, 0, `[${version}]: ${REPO_URL}/compare/v${previous}...v${version}`)
    console.log(`[${version}]: .../compare/v${previous}...v${version} を足した`)
  }
  writeFileSync(CHANGELOG, lines.join('\n'))
}

runScript('bump', 'scripts/bump-version.mjs', [version])
stampChangelog()
runScript('credits', 'scripts/credits.mjs')

if (failures.length > 0) {
  console.error(`\n失敗したステップ:\n${failures.map((f) => `- ${f}`).join('\n')}`)
  process.exit(1)
}
