#!/usr/bin/env node
// リリースのアセットの表（kan/tagpr-scripts#9。kan/pike#586 と同じ形）。**アセットの名前と性質は
// ここにだけ書く。** Release ワークフローの bundle と build-store（最終の名前にそろえる）、attest
// （証明する対象）、upload（そろっているかの検査）と、`updater-json.mjs`（latest.json に載せるもの）が
// この表を引く。
//
//   node scripts/release-assets.mjs collect <bundle|store> <出力先>        出力を最終の名前で写す
//   node scripts/release-assets.mjs check <bundle|store> <ディレクトリ>    そのジョブのアセットがちょうどそろっているか
//   node scripts/release-assets.mjs attest <bundle|store> <ディレクトリ>   証明する対象のパスを 1 行ずつ出す
//   node scripts/release-assets.mjs handoff                                build から bundle へ渡すファイルを 1 行ずつ出す
//
// **bundle は handoff の一覧にあるファイルしか受け取らない。** build のジョブは依存のコードを動かすので、
// その出力（artifact）の中身は信用しない。一覧はこの表から作るので、artifact の中身には左右されない
// （`tauri.conf.json` などを紛れ込ませて上書きできない）。同じ理由で、attest と upload は bundle と
// build-store の artifact を混ぜずに、ジョブごとに表と照らす（依存のコードが動いた build-store が、
// bundle のファイルの名前で差し替えられないように）。
//
// 名前は `updater-json.mjs` が読むものと、これまでのリリースのアセットの名前に合わせてある
// （v0.11.1 まで tauri-action と build-store が上げていた 6 つ。latest.json を足して 7 つ）。
// **変えないこと**（release スキルの確認と attestation の検証も、この名前で探す）。
// 版は `tauri.conf.json` から取る（タグとの一致は Release の draft ジョブが確かめる）。

import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { fail, root } from './release-common.mjs'

const BUNDLE = 'src-tauri/target/release/bundle'
const JOBS = ['bundle', 'store']

/**
 * アセットの一覧。`from` はバンドラ（bundle）と build-store の出力、`name` はリリースでの名前、
 * `job` はそれを作るジョブ。`attest` は利用者が手で落とすもの（`.sig` には付けない）、
 * `updater` は latest.json の platforms のキー（並びは tauri-action の `updaterJsonPreferNsis: true` と
 * 同じ。`windows-x86_64` は NSIS を指す。#147）。
 */
export function releaseAssets(version) {
  const conf = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
  const win = `${conf.productName}_${version ?? conf.version}_x64`
  return [
    {
      job: 'bundle',
      from: `${BUNDLE}/nsis/${win}-setup.exe`,
      name: `${win}-setup.exe`,
      attest: true,
      updater: ['windows-x86_64', 'windows-x86_64-nsis'],
    },
    { job: 'bundle', from: `${BUNDLE}/nsis/${win}-setup.exe.sig`, name: `${win}-setup.exe.sig` },
    // #147 より前に MSI で入れた環境が、NSIS へ移るまで使う
    {
      job: 'bundle',
      from: `${BUNDLE}/msi/${win}_en-US.msi`,
      name: `${win}_en-US.msi`,
      attest: true,
      updater: ['windows-x86_64-msi'],
    },
    { job: 'bundle', from: `${BUNDLE}/msi/${win}_en-US.msi.sig`, name: `${win}_en-US.msi.sig` },
    // Store 版（build-store が作る。アップデータを外したビルドなので latest.json には載せない）
    { job: 'store', from: 'src-tauri/target/release/musql.exe', name: 'muSQL-store-x64.exe', attest: true },
    { job: 'store', from: 'muSQL-store-x64.msix', name: 'muSQL-store-x64.msix', attest: true },
  ]
}

/**
 * build から bundle へ渡すファイル（リポジトリのルートからの相対パス）。バンドラは crate の bin を
 * すべて同梱する。musql の bin は `musql.exe` だけ（testing.md の方針で検証用の bin も作らない）。
 * bin を足したら、ここに足す（足し忘れると `tauri bundle` がファイルの欠けで落ちる）。
 */
export function handoffFiles() {
  return ['src-tauri/target/release/musql.exe']
}

/** そのジョブのアセット。ジョブの名前が違えば止める。 */
function assetsOf(assets, job, usage) {
  if (!JOBS.includes(job)) fail(`usage: ${usage}（受け取った値: ${job}）`)
  return assets.filter((a) => a.job === job)
}

function main([command, ...args]) {
  // handoff は表を使わない（build のジョブで、依存のコードが動いたあとの作業ツリーから呼ぶ）
  if (command === 'handoff') {
    for (const file of handoffFiles()) console.log(file)
    return
  }
  const assets = releaseAssets()
  if (command === 'collect') {
    const [job, out] = args
    const mine = assetsOf(assets, job, 'collect <bundle|store> <出力先>')
    if (!out) fail('usage: collect <bundle|store> <出力先>')
    mkdirSync(out, { recursive: true })
    for (const { from, name } of mine) {
      if (!existsSync(join(root, from))) fail(`${from} がありません（バンドラの出力の名前が変わった？）`)
      copyFileSync(join(root, from), join(out, name))
      console.log(`${from} -> ${name}`)
    }
  } else if (command === 'check') {
    const [job, dir] = args
    const mine = assetsOf(assets, job, 'check <bundle|store> <ディレクトリ>')
    if (!dir) fail('usage: check <bundle|store> <ディレクトリ>')
    if (!existsSync(dir)) fail(`${dir} がありません（artifact を落とせていない？）`)
    const have = readdirSync(dir).sort()
    const want = mine.map((a) => a.name).sort()
    if (have.join('\n') !== want.join('\n')) {
      fail(`${job} のアセットが表と合いません\n  ある: ${have.join(', ')}\n  表:   ${want.join(', ')}`)
    }
    console.log(`ok: ${job} ${have.length} 件`)
  } else if (command === 'attest') {
    const [job, dir] = args
    const mine = assetsOf(assets, job, 'attest <bundle|store> <ディレクトリ>')
    if (!dir) fail('usage: attest <bundle|store> <ディレクトリ>')
    if (!existsSync(dir)) fail(`${dir} がありません（artifact を落とせていない？）`)
    // actions/attest の subject-path に渡すので `/` でつなぐ
    for (const { name } of mine.filter((a) => a.attest)) console.log(`${dir}/${name}`)
  } else {
    fail('usage: node scripts/release-assets.mjs <collect|check|attest|handoff> ...')
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2))
}
