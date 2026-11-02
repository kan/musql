#!/usr/bin/env node
// 「muSQL について」に出すライセンス一覧（ui/credits.json、#119）を作り直す。
//
//   node scripts/credits.mjs        （just credits）
//
// ふだんはリリース PR の release-prepare.mjs が呼ぶ（#149）。そのジョブは書き込みの権限を
// 持つので、musql も依存もビルドしない。使うのは `cargo metadata`（依存のソースはレジストリへ
// 展開されるだけで、build.rs は走らない）と、SPDX の標準文の取得だけ。
//
// 載せるもの:
// - リリースするターゲット（TARGET）と既定の feature で、musql から通常の依存（build と dev
//   以外）でたどれるクレート。proc-macro も通常の依存なので入る（出荷するものより多くはなっても、
//   少なくはならない）。Store 版は feature を外すだけなので、その依存はこの一覧の部分集合になる。
//   **リリースするターゲットを増やす、または default に入らない feature を足すときは、ここも
//   合わせる**（合わせないと、その分の依存が一覧から漏れ、何も落ちない）
// - ui/lib/ の vendor と、ui/icons.js に写した Lucide のアイコン。scripts/vendored.json に手で
//   並べる（Lucide は ui/lib/lucide にライセンスだけを置いている。写したときの版は記録が無い）
//
// ライセンスの本文は、各パッケージの LICENSE / COPYING / NOTICE などのファイル（とマニフェストの
// license-file）から読み、同じ本文は 1 つにまとめる。本文を持たないパッケージには、そのライセンスの
// SPDX の標準文を付け、`standard` と作者を添える（cargo-about と同じ）。
//
//   node scripts/credits.mjs --check        （just check-credits）
//
// は書き出さずに、コミット済みの一覧が今の依存と合っているかだけを見る（ネットワークに出ない）。
// クレートは名前とライセンスだけを比べ、版は比べない（版は作り直すたびに合う）。vendor は版も
// 比べる（dependabot が触らないので、食い違いは vendored.json を直して作り直し忘れたときだけ）。
// just test と CI には入れていない: dependabot の PR がクレートを足すと落ち、直すには dependabot の
// ブランチへ push するしかなくなる（すると dependabot が以後 rebase しなくなる）。

import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, sep } from 'node:path'
import { fail, root } from './release-common.mjs'

const OUT = 'ui/credits.json'

/** リリースするターゲット（release.yml は windows-latest で `--target` を付けずにビルドする）。 */
const TARGET = 'x86_64-pc-windows-msvc'

/** 標準文の取得元。作り直しで本文が変わらないよう、リリースで固定する。 */
const SPDX_TEXT = 'https://raw.githubusercontent.com/spdx/license-list-data/v3.27.0/text'

/** ライセンスを宣言していないパッケージに出す表記（その本文を読んでもらう）。 */
const UNKNOWN_LICENSE = 'see license text'

/**
 * これより短い本文は、ライセンスを指す注記であってライセンスそのものではない（UTF-8 のバイト数）。
 * 載せている中でいちばん短い ISC と 0BSD はこれより長い。この値のきっかけになった注記は 280 文字。
 */
const FULL_TEXT_MIN = 500

/** 出荷するクレート。 */
function shippedCrates() {
  const r = spawnSync(
    'cargo',
    ['metadata', '--format-version', '1', '--locked', '--filter-platform', TARGET],
    { cwd: join(root, 'src-tauri'), encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
  )
  if (r.status !== 0) fail(`credits: cargo metadata が失敗しました:\n${r.stderr ?? r.error}`)
  const meta = JSON.parse(r.stdout)
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]))
  const packages = new Map(meta.packages.map((p) => [p.id, p]))

  // musql から通常の依存（kind が null）だけをたどる。build と dev の先は出荷しない
  const rootId = meta.resolve.root
  const seen = new Set()
  const stack = [rootId]
  while (stack.length > 0) {
    const id = stack.pop()
    if (seen.has(id)) continue
    seen.add(id)
    for (const dep of nodes.get(id).deps ?? []) {
      if ((dep.dep_kinds ?? []).some((k) => k.kind === null)) stack.push(dep.pkg)
    }
  }
  seen.delete(rootId)

  const nonEmpty = (v) => (typeof v === 'string' && v !== '' ? v : null)
  // パッケージ ID の順に回す。本文の番号は最初に出てきた順に振るので、順番が変わると
  // 中身が同じでもファイルの差分になる
  return [...seen].sort().map((id) => {
    const p = packages.get(id)
    const dir = dirname(p.manifest_path)
    const licenseFile = nonEmpty(p.license_file)
    return {
      name: p.name,
      version: p.version,
      license: nonEmpty(p.license),
      // 正規化しない（同じファイルを別のパスとして数えないよう、ディレクトリ内の名前と同じ形で並べる）
      licenseFile: licenseFile === null ? null : `${dir}${sep}${licenseFile}`,
      // 画面は URL をブラウザで開き、そこでは https だけを許す。それ以外は crates.io のページにする
      url:
        [p.repository, p.homepage].map(nonEmpty).find((u) => u?.startsWith('https://')) ??
        `https://crates.io/crates/${p.name}`,
      authors: p.authors ?? [],
      dir,
    }
  })
}

/** LICENSE / LICENSE-MIT / COPYING.txt / NOTICE / UNLICENSE / COPYRIGHT。大文字小文字と英綴りを問わない。 */
function isLicenseFile(name) {
  const lower = name.toLowerCase()
  return ['license', 'licence', 'copying', 'notice', 'unlicense', 'copyright'].some((stem) => {
    if (!lower.startsWith(stem)) return false
    const rest = lower.slice(stem.length)
    return rest === '' || ['-', '.', '_'].includes(rest[0])
  })
}

/** 保存する形：行末は \n、前後の空白を除く（2 つのパッケージの同じライセンスを同じ本文にする）。 */
function normalise(text) {
  return text.replaceAll('\r\n', '\n').replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '')
}

const isFile = (path) => {
  try {
    return statSync(path).isFile()
  } catch {
    return false
  }
}

/** クレートが持つ本文：マニフェストが指すファイル、続けてライセンスらしい名前のファイルを名前順に。 */
function shippedTexts(c) {
  const files = c.licenseFile === null ? [] : [c.licenseFile]
  let names = []
  try {
    names = readdirSync(c.dir)
  } catch {
    // 読めないディレクトリは本文なしとして扱う（標準文に落ちる）
  }
  const byName = names
    .filter(isLicenseFile)
    .map((n) => `${c.dir}${sep}${n}`)
    .sort()
  // license-file が `./LICENSE` や `/` 区切りでも、同じファイルを 2 回数えない（Rust の Path の比較と同じ）
  const same = (p) =>
    p
      .split(/[\\/]/)
      .filter((part) => part !== '.' && part !== '')
      .join('/')
  for (const path of byName) {
    if (!files.some((f) => same(f) === same(path))) files.push(path)
  }
  // ファイルでないもの（LICENSES というディレクトリ、無いファイルを指す license-file）は落とす
  return files
    .filter(isFile)
    .map((path) => normalise(readFileSync(path).toString('utf8')))
    .filter((t) => t !== '')
}

/** ライセンス式の SPDX の識別子。OR の選択肢はどれを選んだとも言えないので、すべて返す。 */
function spdxIds(expression) {
  return expression
    .replace(/[()/]/g, ' ')
    .split(/\s+/)
    .filter((w) => w !== '' && !['OR', 'AND', 'WITH'].includes(w))
}

const texts = []
const textIndex = new Map()

/** 本文を 1 つだけ保存し、その番号を返す（番号は最初に出てきた順）。 */
function intern(text) {
  let i = textIndex.get(text)
  if (i === undefined) {
    i = texts.push(text) - 1
    textIndex.set(text, i)
  }
  return i
}

async function fetchStandardText(id) {
  const url = `${SPDX_TEXT}/${id}.txt`
  const res = await fetch(url).catch((e) => fail(`credits: ${id} の標準文を取得できません（${url}）: ${e}`))
  if (!res.ok) fail(`credits: ${id} の標準文を取得できません（${url}）: HTTP ${res.status}`)
  return normalise(await res.text())
}

const vendored = JSON.parse(readFileSync(join(root, 'scripts/vendored.json'), 'utf8'))

/**
 * `--check`：コミット済みの一覧と今の依存を、`名前 | ライセンス`（vendor は `名前 版 | ライセンス`）で比べる。
 * 本文の番号が実在するかも見る。
 */
function check(crates) {
  const credits = JSON.parse(readFileSync(join(root, OUT), 'utf8'))
  const problems = []
  const listed = new Set()
  for (const p of credits.packages) {
    if (p.texts.length === 0) problems.push(`${p.name} にライセンスの本文が無い`)
    for (const i of p.texts) {
      if (!(i < credits.texts.length)) problems.push(`${p.name} が存在しない本文 ${i} を指している`)
    }
    listed.add(p.vendored ? `${p.name} ${p.version} | ${p.license}` : `${p.name} | ${p.license}`)
  }
  const expected = new Set([
    ...crates.map((c) => `${c.name} | ${c.license ?? UNKNOWN_LICENSE}`),
    ...vendored.map((v) => `${v.name} ${v.version} | ${v.license}`),
  ])
  const missing = [...expected].filter((e) => !listed.has(e))
  const stale = [...listed].filter((e) => !expected.has(e))
  if (missing.length > 0) problems.push(`まだ載っていない: ${missing.join(', ')}`)
  if (stale.length > 0) problems.push(`もう出荷していない: ${stale.join(', ')}`)
  if (problems.length > 0) {
    fail(`credits: ${OUT} が今の依存と合っていません。just credits で作り直してください。\n- ${problems.join('\n- ')}`)
  }
  console.log(`credits: ${OUT} は今の依存と合っている`)
}

/** キーを辞書順に並べ直す（serde_json の既定の書き出しと同じ並び）。 */
function sortKeys(value) {
  if (Array.isArray(value)) return value.map(sortKeys)
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((k) => [k, sortKeys(value[k])]),
    )
  }
  return value
}

const crates = shippedCrates()
if (process.argv.includes('--check')) {
  check(crates)
  process.exit(0)
}

// SPDX の識別子 → 標準文の番号（1 つの識別子は 1 回だけ取る）
const standard = new Map()
const packages = []

for (const c of crates) {
  const own = shippedTexts(c).map(intern)
  const entry = { name: c.name, version: c.version, license: c.license ?? UNKNOWN_LICENSE, url: c.url }
  // パッケージが短い注記（"licensed under MIT or Apache-2.0, see ..."）しか持たないことがある。
  // それは本文を持っているとはみなさない
  const hasFullText = own.some((i) => Buffer.byteLength(texts[i]) >= FULL_TEXT_MIN)
  if (hasFullText) {
    entry.texts = own
  } else {
    const ids = c.license === null ? [] : spdxIds(c.license)
    if (ids.length === 0 && own.length === 0) {
      fail(
        `credits: ${c.name} ${c.version} はライセンスのファイルを持たず、ライセンスも宣言していません。` +
          '自動では載せられないので、ライセンスを手で調べてください。',
      )
    }
    // 持っている注記のあとに、ライセンスの標準文を足す
    for (const id of ids) {
      if (!standard.has(id)) {
        console.log(`credits: ${id} の標準文を取得する`)
        standard.set(id, intern(await fetchStandardText(id)))
      }
      const index = standard.get(id)
      if (!own.includes(index)) own.push(index)
    }
    entry.texts = own
    entry.standard = true
    entry.authors = c.authors
  }
  packages.push(entry)
}

for (const v of vendored) {
  const path = join(root, v.file)
  if (!existsSync(path)) fail(`credits: ${v.name} のライセンスファイル ${v.file} がありません`)
  packages.push({
    name: v.name,
    version: v.version,
    license: v.license,
    url: v.url,
    vendored: true,
    texts: [intern(normalise(readFileSync(path, 'utf8')))],
  })
}

// 大文字小文字を区別せずに並べる（"CodeMirror" をクレートの前ではなく間に置く）
const key = (p) => [p.name.toLowerCase(), p.version]
packages.sort((a, b) => {
  const [an, av] = key(a)
  const [bn, bv] = key(b)
  if (an !== bn) return an < bn ? -1 : 1
  return av < bv ? -1 : av > bv ? 1 : 0
})

writeFileSync(join(root, OUT), `${JSON.stringify(sortKeys({ packages, texts }), null, 2)}\n`)
console.log(`credits: ${OUT} を書き出した（パッケージ ${packages.length}、本文 ${texts.length}）`)
