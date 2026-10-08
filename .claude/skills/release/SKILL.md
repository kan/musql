---
name: release
description: muSQL の新しいバージョンをリリースする（dependabot の取り込み → CHANGELOG → bump → push → タグ → ドラフトのアセット確認 → リリースノートを書いて公開）。「リリースして」「vX.Y.Z を出して」と頼まれたときに使う。
---

# muSQL のリリース手順

**リリースの依頼は end-to-end の依頼。** バージョン bump のコミットだけでなく、`main` の push、タグの作成と push、Release ワークフローの完了待ち、ドラフトのアセット確認、リリースノートの記載と公開までを Claude が実行する。通常のコミット運用と違い、push の個別確認は要らない（CLAUDE.md「Git workflow」の例外規定）。以下を順番に最後まで実行する。

バージョン番号は変更内容から判断する（新機能ならマイナー、修正のみならパッチ）。**判断に迷う場合と、ユーザーが番号を指定していない大きめの変更では、ユーザーに確認する。**

## 0. dependabot の PR を取り込む

**バージョン bump より前に、open な dependabot の PR を片付ける。** 出荷済みの版に後から依存の更新は混ぜられないので、残したまま出すと次のリリースまで届かない。

```
gh pr list --author app/dependabot --state open
```

- PR は ecosystem ごとに 1 つにまとまっている（`.github/dependabot.yml` の `groups`）。CI が通っていて破壊的変更が無ければ、そのままマージする
- CI が落ちている、または破壊的変更を含むときは、ローカルで取り込んで直す（`cargo update -p <クレート>`、必要ならコードの修正）。main に同じ更新が入ると、PR は dependabot が自動で閉じる
- russh や Tauri 本体など、接続やウィンドウに関わる依存を上げたときは、ユーザーの動作確認を取ってから先へ進む
- 取り込んだら `just check` を通し、push して main の CI が通ることを確認する

open な PR が無ければ、この節は飛ばす。

## 1. CHANGELOG

**書く対象は、前回のタグからの差分で洗い出す。**

```
git log --oneline "$(git describe --tags --abbrev=0)"..HEAD
```

**記憶で書かないこと。** そのセッションで対応したものを思い出して並べると、前回のリリース以降に積まれていた他の変更が落ちる。公開してから直しても、既に読んだ人には届かない。

洗い出した一覧から、`CHANGELOG.md` の先頭に新しいセクションを足す（日付、Added / Changed / Fixed / Security、末尾の比較リンク）。同じ一覧を 6 のリリースノートにも使う。

足した節には、CLAUDE.md「ドキュメント校正ルール」の校正をかける。過去の節は出荷済みの記録なので触らない。

## 2. バージョン番号の更新

```
just bump X.Y.Z
```

`src-tauri/Cargo.toml` と `src-tauri/tauri.conf.json` の `version` を更新し、`cargo check` で `Cargo.lock` の `musql` エントリまで追従させる。**手で編集しない**（lockfile の drift が残り、後から同期コミットが必要になる）。

`store/AppxManifest.xml` は `Version="{{VERSION}}"` のプレースホルダで、CI がタグから流し込むので編集しない。

## 3. 検証してコミット

`just check`（fmt / clippy / test / UI lint）を通してから:

```
git add CHANGELOG.md src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
git commit -m "Bump version to X.Y.Z"
```

## 4. push とタグ

```
git push origin main
git tag vX.Y.Z && git push origin vX.Y.Z
```

**タグを打つ前に必ずバージョンを更新すること。** `tauri-action` は `tauri.conf.json` の `version` をアセット名に埋め込むので、ずれると `latest.json` の指す先と実ファイル名が食い違う。

タグを打ち直す場合は、先にドラフトの Release を消す（残すと、打ち直したタグのビルドが古いドラフトへアセットを足す）:

```
gh release delete vX.Y.Z --yes
git push origin :refs/tags/vX.Y.Z && git tag -d vX.Y.Z
```

修正してから、もう一度タグを打つ。

## 5. ワークフローの完了待ちとアセットの確認

タグ push で `Release` ワークフローが起動する。ジョブは 2 つで、**`build` → `build-store` の順に直列で走る**。両方の完了を待つ。

- `build` … NSIS と MSI のインストーラ、`latest.json`（セルフアップデータ用）をビルドし、**ドラフトの Release を作る**
- `build-store` … Store 用の EXE と MSIX をビルドし、同じドラフトへ追加する

Windows のフルビルドが 2 回なので、合わせて 20〜40 分かかる。`gh run watch <id> --exit-status` をバックグラウンドで走らせて待つ（ポーリングを前景で回さない）。

完了したら、**公開する前に**ドラフトのアセットを確認する:

```
gh release view vX.Y.Z --json isDraft,assets --jq '.isDraft, (.assets[].name)'
```

期待するアセットは次の 7 つ（v0.7.1 の実績）。

- NSIS インストーラ（`muSQL_X.Y.Z_x64-setup.exe`）とその署名（`.exe.sig`）
- MSI インストーラ（`muSQL_X.Y.Z_x64_en-US.msi`）とその署名（`.msi.sig`）
- `latest.json`
- `muSQL-store-x64.exe`
- `muSQL-store-x64.msix`

**`latest.json` が付いているか必ず確認する。** 無いまま公開すると、セルフアップデータが黙って壊れる（`tauri-action` v1.0.0 で `includeUpdaterJson` が `uploadUpdaterJson` に改名された経緯があり、設定漏れが起きやすい）。足りないものがあれば公開せず、原因を直してタグを打ち直す。

署名には GitHub Secrets の `TAURI_SIGNING_PRIVATE_KEY` と `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` が要る（未署名のビルドは updater の検証に失敗する）。

## 6. リリースノートを書いて公開する

ドラフトの本文は "See the assets below to download and install." の固定文になっている。**1 で CHANGELOG に足した節の内容で上書きし、同時に公開する**（別々に書くと、片方にしか無い項目ができる）。

本文はファイルに書いて渡す。`--notes "..."` に直接書くと、バッククォートや `$` をシェルが解釈して本文が壊れる。

```
gh release edit vX.Y.Z --draft=false --notes-file <本文を書いたファイル>
```

**公開するまで、利用者には何も配られない。** ドラフトは `releases/latest` にならないので、セルフアップデータの確認先（`releases/latest/download/latest.json`）は前の版を返し続ける。逆に言うと、公開した時点で全利用者への配布が始まる。アセットの確認を済ませてから公開すること。

## 7. 公開後の確認

```
gh release list --limit 1 --json tagName,isDraft,isLatest
```

先頭が今回のタグで、`isDraft` が `false`、`isLatest` が `true` になっていること。あわせて、リリースした版で閉じる issue が残っていないかを見る。

Store 版の提出（Partner Center への MSIX のアップロード）は、このスキルの範囲外。ユーザーが行う。
