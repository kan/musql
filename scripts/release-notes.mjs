#!/usr/bin/env node
// CHANGELOG.md の節から、リリースノートの本文を作る（release.yml の draft ジョブ、#149）。
//
//   node scripts/release-notes.mjs vX.Y.Z > notes.md
//
// ドラフトの本文になり、tauri-action が latest.json の notes にも入れる。latest.json は公開前に
// 上がり、公開後は Immutable releases で差し替えられないので、本文はタグの時点の CHANGELOG から
// 作る。次のときは止める:
// - タグが vX.Y.Z の形でない、または tauri.conf.json の版と食い違う（成果物の名前はこの版から付く）
// - 節が無いか、2 つ以上あるか、空（書き忘れたままタグを打ったとき）

import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fail, headings, readChangelog, root, sectionBody, VERSION_RE } from './release-common.mjs'

const tag = process.argv[2] ?? ''
const version = tag.replace(/^v/, '')
if (!tag.startsWith('v') || !VERSION_RE.test(version)) fail('usage: node scripts/release-notes.mjs vX.Y.Z')

const configured = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8')).version
if (configured !== version) fail(`タグ ${tag} が tauri.conf.json の版（${configured}）と食い違っています`)

const lines = readChangelog()
const starts = headings(lines).filter((i) => lines[i].startsWith(`## [${version}]`))
if (starts.length === 0) fail(`CHANGELOG.md に ${version} の節がありません（見出しは "## [${version}] - 日付"）`)
if (starts.length > 1) fail(`CHANGELOG.md に ${version} の節が ${starts.length} つあります`)
const body = sectionBody(lines, starts[0])
if (!body) fail(`CHANGELOG.md の ${version} の節が空です`)

process.stdout.write(`${body}\n`)
