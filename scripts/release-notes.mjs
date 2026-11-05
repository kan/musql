#!/usr/bin/env node
// CHANGELOG.md の節から、リリースノートの本文を作る（release.yml の draft ジョブ、#149）。
//
//   node scripts/release-notes.mjs vX.Y.Z > notes.md
//
// ドラフトの本文になり、tauri-action が latest.json の notes にも入れる。latest.json は公開前に
// 上がり、公開後は Immutable releases で差し替えられないので、本文はタグの時点の CHANGELOG から
// 作る。タグが vX.Y.Z の形でない、または tauri.conf.json の版と食い違うときは止める（成果物の
// 名前はこの版から付く）。
//
// 節の抜き出しは kan/tagpr-scripts の subtree（`scripts/tagpr/release-notes.mjs`、#152）に任せる。
// 節が無いか、2 つ以上あるか、空なら、そちらが止める。段落と項目の中の折り返しは 1 行につなぐ。

import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fail, root, runNode, VERSION_RE } from './release-common.mjs'

const tag = process.argv[2] ?? ''
const version = tag.replace(/^v/, '')
if (!tag.startsWith('v') || !VERSION_RE.test(version)) fail('usage: node scripts/release-notes.mjs vX.Y.Z')

const configured = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8')).version
if (configured !== version) fail(`タグ ${tag} が tauri.conf.json の版（${configured}）と食い違っています`)

process.exit(runNode('scripts/tagpr/release-notes.mjs', [tag]))
