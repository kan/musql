---
name: release
description: muSQL の新しいバージョンをリリースする（dependabot の取り込み → tagpr のリリース PR の確認 → CI でスクリーンショット → リリース PR のマージでタグとビルド → ドラフトの確認と公開）。「リリースして」「vX.Y.Z を出して」と頼まれたときに使う。
---

# muSQL のリリース手順

**リリースの依頼は end-to-end の依頼。** リリース PR の確認と調整、スクリーンショットの撮影、リリース PR のマージ、Release の完了待ち、ドラフトの確認と公開までを Claude が実行する。通常の PR 運用と異なり、リリース PR のマージの個別確認は要らない（CLAUDE.md「Git workflow」の例外規定）。CI の完了待ちは、バックグラウンドで `gh run watch` を走らせて待つ（前景でポーリングしない）。以下を順番に最後まで実行する。

**版、`Cargo.lock`、CHANGELOG の見出しとリンク、ライセンス一覧は、tagpr が作るリリース PR に載る**（#149）。main に push があるたびに `tagpr.yml` がリリース PR を作り直し、`scripts/release-prepare.mjs` が中身をそろえる。仕組みと理由は CLAUDE.md の「リリース PR（tagpr）」。**版のファイル（`tauri.conf.json`、`Cargo.toml`、`Cargo.lock`）を手で書き換えない。リリース PR のブランチへ手でコミットしない**（画像のコミットは手順 4 のワークフローが積む）。

## 0. dependabot の PR を取り込む

**リリース PR をマージする前に、open な dependabot の PR を片付ける。** 出荷済みの版に後から依存の更新は混ぜられないので、残したまま出すと次のリリースまで届かない。

```bash
gh pr list --author app/dependabot --state open
```

- PR は ecosystem ごとに 1 つにまとまっている（`.github/dependabot.yml` の `groups`）。CI が通っていて破壊的変更が無ければ、そのまま squash でマージする
- CI が落ちている、または破壊的変更を含むときは、マージしてから直す PR を作る（**dependabot のブランチへ手で push しない**。CLAUDE.md「Git workflow」）
- russh や Tauri 本体など、接続やウィンドウに関わる依存を上げたときは、ユーザーの動作確認を取ってから先へ進む
- 利用者に届く依存の更新（cargo）は、`CHANGELOG.md` の `## [Unreleased]` の `### Changed` に「依存関係を更新（主なもの）」を 1 行足す PR を作る。セキュリティの修正を含むなら `### Security` に書く。**ライセンス一覧は作り直さない**（リリース PR が作り直す）

0 の PR をマージしたら、tagpr がリリース PR を作り直す。次へ進む前に、その run の完了を待つ。

## 1. リリース PR を確かめる

```bash
gh pr list --label tagpr --state open --json number,title,headRefName
gh run list --workflow tagpr.yml --branch main -L 3
```

**ブランチ名は `tagpr-from-v<前回の版>`**（v0.12.0 を出すリリース PR なら `tagpr-from-v0.11.1`）。以下の `<ブランチ>` は上の一覧に出るブランチ名を使う。

**最新の tagpr の run が success であること。** 落ちた run はリリース PR を draft にする（次に成功した run が ready に戻す）。失敗していたら、run の注釈（`postVersionCommand` の失敗）を読む。多いのは `## [Unreleased]` が空のとき。`tauri.conf.json` か `Cargo.toml` の版が提案と違う（`says X, but tagpr proposes Y`）と言われたら、そのファイルの版のキーより前に今の版と同じ文字列がある（CLAUDE.md の「リリース PR（tagpr）」）。直す PR を main にマージすると作り直される。

**版が正しいか**（PR のタイトルの `vX.Y.Z`）。機能の PR（`enhancement`）があれば minor、無ければ patch になる。違うときは `tauri.conf.json` も `Cargo.toml` も手で直さず、ラベルを付けて作り直す:

```bash
gh pr edit <番号> --add-label tagpr:minor
gh workflow run tagpr.yml --ref main
```

Files changed が、版の表記（`tauri.conf.json` / `Cargo.toml` / `Cargo.lock`）、`ui/credits.json`、`CHANGELOG.md` の見出しとリンク（版の節の見出し、その上に戻した空の `## [Unreleased]`、末尾の `releases/tag` のリンク）だけであることを確かめる。

**リリース PR にも Copilot のレビューが付く**（ルールセットの条件は PR の向き先でしか絞れないため）。差分は上の生成物だけで、コードの中身は各 PR で見てあるので、指摘は読むだけにする。版の食い違いのような生成物の誤りを指していたら、手で直さずに原因（`.tagpr` の `versionFile`、`scripts/release-prepare.mjs`、main の CHANGELOG か `Cargo.toml`）を直す PR を作る。原因が `scripts/tagpr/` の中なら、上流の kan/tagpr-scripts を直して subtree pull する。

## 2. CHANGELOG の節を確かめる

**この節がそのままリリースノートになる。** マージすると `draft` ジョブが `scripts/release-notes.mjs` で節を抜き出してドラフトの本文にし、`build` がそれを `latest.json` の `notes` に入れる。**`latest.json` の `notes` はタグの時点で決まり、公開後は差し替えられない**ので、マージの前に仕上げる。

```bash
git fetch origin
git show origin/<ブランチ>:CHANGELOG.md | sed -n '1,40p'
git log --oneline "$(git describe --tags --abbrev=0 origin/main)"..origin/main
```

**記憶で照合しないこと。** `git log` の各コミット（PR）が節に載っているか、利用者に届かない変更（CI、開発ノートなど）だけが抜けているかを見る。載っていない利用者向けの変更や、言い回しの直しは、**main への PR で `## [Unreleased]` を直す**（リリース PR のブランチでは見出しが書き換わっているので、そこへコミットすると tagpr の作り直しとぶつかる）。マージ後に作り直されたリリース PR で見直す。**公開してからでは、直しても既に読んだ人には届かない。** 校正は CLAUDE.md の「ドキュメント校正ルール」（各 PR で済ませてあるが、節全体のつながりもここで見る）。

## 3. CI の確認

**リリース PR の run は、ボット（GITHUB_TOKEN）が作った PR なので承認待ち（`action_required`）で止まる。** 撮影のコミット（手順 4）にも run ができる。**今の head の run（CI と Verify Lockfile）だけを API で承認し**、通ることを確かめる。古い head の run は承認しなくてよい。**CodeQL の run は待たない**（解析する言語のファイルが変わったときだけ回る。リリース PR が変えるのは版、`Cargo.lock`、CHANGELOG、ライセンス一覧、画像だけなので、run ができない。#157）:

```bash
gh run list --branch <ブランチ> -L 6 --json databaseId,workflowName,conclusion,headSha
gh api -X POST repos/kan/musql/actions/runs/<id>/approve   # それぞれ
gh run watch <id> --exit-status
```

main の最新の CI と CodeQL も success であること。

## 4. スクリーンショットの撮り直し

**UI を変えた版、またはマイナーの版では撮り直す。** **bump 済みのリリース PR のブランチで撮る。** CHANGELOG 以外の PR を main にマージし終えてから起動する（UI が変わると撮り直しになる）。

```bash
gh workflow run screenshots.yml --ref <ブランチ>   # 既定で commit: true。起動した run の URL を出す
gh run watch <URL の末尾の run id> --exit-status
```

**run の id は `gh workflow run` が出す URL から取る。** 起動の直後に `gh run list -L 1` で引くと、新しい run が一覧に現れる前で、前回の完了済みの run を拾うことがある。

ジョブは撮影 → `just screenshots-sync` → 差分があればリリース PR のブランチへ `vX.Y.Z でマニュアルのスクリーンショットを撮り直す` を積む。**差分が無ければ何も積まない。** **撮影中に tagpr がブランチを作り直すと push で落ちる**（古いツリーの画像を載せないため）。起動し直す。

1. リリース PR の Files changed で画像を目視で確かめる（その回で変えた UI が写っているか）
2. 撮り直しが要るなら、直す PR を main にマージし、作り直されたリリース PR のブランチでもう一度起動する
3. 画像のコミットで CI の run が増えるので、手順 3 の承認をその head でもう一度行う

## 5. リリース PR のマージ

CHANGELOG の見出しの日付は、tagpr が最後にリリース PR を作り直した日になる。**マージする日と違えば、先に作り直して日付を合わせる**（`gh workflow run tagpr.yml --ref main`。画像のコミットは引き継がれる）。

```bash
gh pr merge <番号> --squash --delete-branch
git switch main && git pull --ff-only
sha=$(git rev-parse HEAD)
gh run list --workflow tagpr.yml --commit "$sha" --json databaseId,status -q '.[0]'
gh run watch <run id> --exit-status
```

マージの push で走る tagpr の run が、**タグ `vX.Y.Z` を打ち、annotated に打ち直してから、同じ run の `release` ジョブで Release ワークフローをそのタグで起動する**（どれも kan/tagpr-scripts の共通の workflow が行う。`workflow_dispatch`。GITHUB_TOKEN で打ったタグの push では Release は起動しない）。Release は tagpr とは別の run になる。中は `draft`（ドラフトを 1 つ作る）→ `build`（コンパイルだけ）と `build-store` が並列 → `bundle`（インストーラと `.sig`）→ `attest` と `upload` → `updater-json`（`latest.json`）の順に走る。ジョブの分け方は CLAUDE.md の「Build variants」。Windows のリリースビルド 2 回が並列なので、20 分ほどかかる。

```bash
gh run list --workflow release.yml --branch vX.Y.Z -L 1 --json databaseId,status
gh run watch <run id> --exit-status
```

**マージの run で `credits` ジョブだけが赤なら**（SPDX の取得の一時的な失敗など）、run は赤になるが、タグと Release の起動には影響しない（呼ぶジョブは `credits` が落ちても走る。マージの run はライセンス一覧を使わない）。タグが打たれ、Release の run が始まっていれば、そのまま進む。再実行しない。

**マージの tagpr の run が cancelled になっていたら**（待っている間に main へ push が来ると、GitHub は古いほうを取り消す）、タグは打たれていない。その run を再実行する（`gh run rerun <run id>`。マージコミットをチェックアウトしてタグを打つ）。後の push の tagpr の run は「タグより新しい版の節がある」で赤になっているが、再実行でタグが打たれれば次の run から通る。**新しいリリース PR をマージしてやり過ごさない**（打たれていない版の上に次の版を重ねることになる）。置き換えた側の run は赤になり、版だけ上がったリリース PR が draft で残ることがある。**その PR はマージしない。** タグが打たれた後、main への push で走る次の成功した run が、コメントを付けて閉じる（`scripts/tagpr/README.md` の「The tagpr workflow」）。閉じられた PR を開き直してマージしない。

**タグは打たれたのに Release の run が無いとき**（annotated に打ち直すステップか、Release の起動が落ちた）は、**そのマージの tagpr の run を再実行する**（`gh run rerun <run id>`。tagpr はマージコミットに付いた既にあるタグを受け入れて、続きをもう一度走らせる）。**Release の run が既に始まっている run は再実行しない**（同じタグで Release が 2 回起動する）。30 日を過ぎて再実行できないときの手での直し方は、`scripts/tagpr/README.md` の「The tagpr workflow」にある。musql の `release.yml` は `push: tags` を持たないので、手でタグを打ち直したあとも `gh workflow run release.yml --ref vX.Y.Z` で起動する。

**タグが打たれ、Release の run のジョブがすべて success であることを確かめる。**

- `draft` が落ちたら、タグで起動されているか、タグが `vX.Y.Z` の形か、`tauri.conf.json` の version と合っているか、タグのコミットの CHANGELOG に `## [X.Y.Z] - 日付` の節が 1 つだけあり空でないかを見る
- ビルドのどれかが落ちたら、一時的な失敗なら落ちたジョブだけを再実行する（`gh run rerun <run-id> --failed`。ジョブの間で渡す artifact は 30 日残る）
- `bundle` の `Take the build output` が落ちたら、`build` の出力が `scripts/release-assets.mjs handoff` の一覧と合っていない（bin を足した、など）。`Collect the release assets` が落ちたら、バンドラの出力の名前が表と合っていない。どちらも表かワークフローを直す PR を作り、タグを打ち直す
- 打ってあるタグで丸ごとやり直すなら `gh workflow run release.yml --ref vX.Y.Z`（**`--ref` にはタグを渡す**。ブランチで起動すると `draft` が止める。`draft` が同じタグのドラフトを消してから作り直す）
- コードや CHANGELOG を直す必要があるなら、下の「タグを打ち直す場合」

## 6. ドラフトの確認

```bash
gh release view vX.Y.Z --json isDraft,assets --jq '.isDraft, (.assets[].name)'
```

期待するアセットは次の 7 つ。

- NSIS インストーラ（`muSQL_X.Y.Z_x64-setup.exe`）とその署名（`.exe.sig`）
- MSI インストーラ（`muSQL_X.Y.Z_x64_en-US.msi`）とその署名（`.msi.sig`）
- `latest.json`
- `muSQL-store-x64.exe`
- `muSQL-store-x64.msix`

**`latest.json` が付いているか必ず確認する。** 無いまま公開すると、セルフアップデータが黙って壊れる（`updater-json` ジョブは `attest` と `upload` の両方が通ったときだけ走るので、どちらかが落ちると付かない）。中身の `windows-x86_64` が NSIS の setup.exe を指し、`notes` が CHANGELOG の節になっていることも見る。足りないものがあれば公開せず、原因を直してタグを打ち直す。

署名には GitHub Secrets の `TAURI_SIGNING_PRIVATE_KEY` と `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` が要る（未署名のビルドは updater の検証に失敗する）。読むのは `bundle` ジョブだけ。`release.yml` はリポジトリの secrets を直接読む（`workflow_dispatch` で起動するため）。`updater-json` は、上がった `.sig` を `tauri.conf.json` の公開鍵で検証してから `latest.json` を書くので、鍵が食い違っていればそこで落ちる。

**手でダウンロードする 4 つのファイルに attestation（来歴の証明）が付いていることを確認する**（#130）。ドラフトから落として 1 つずつ検証する:

```bash
dir=$(mktemp -d)
gh release download vX.Y.Z --repo kan/musql --dir "$dir" \
  --pattern '*-setup.exe' --pattern '*.msi' \
  --pattern 'muSQL-store-x64.exe' --pattern 'muSQL-store-x64.msix'
ls "$dir"   # 4 件あること
for f in "$dir"/*; do
  gh attestation verify "$f" --repo kan/musql \
    --signer-workflow kan/musql/.github/workflows/release.yml \
    --source-ref refs/tags/vX.Y.Z || echo "FAILED: $f"
done
```

**4 つとも通ること。** `attest` ジョブは `scripts/release-assets.mjs` の表のファイルを名指しで渡し、表とそろっていなければ落ちるが、検証は配るファイルそのもので確かめる最後の確認になる。1 つでも落ちたら公開せず、直してタグを打ち直す。Release はタグで起動されるので、source の ref はタグになる（#152。v0.11.1 までは `workflow_call` で呼んでいたので `refs/heads/main`。それらの版を検証するときは `--source-ref` を外す）。

## 7. 公開

ドラフトの本文（CHANGELOG の節から作られている）を確かめて公開する。**本文は手で書き直さない**（手で直した本文は `latest.json` の `notes` に届かず、CHANGELOG ともずれる）。直したいところがあれば、公開せずに CHANGELOG を直す PR をマージし、**タグを打ち直す**（本文と `notes` はタグのコミットの CHANGELOG から作られるので、ワークフローの再実行だけでは変わらない）:

```bash
gh release view vX.Y.Z --json body -q .body
gh release edit vX.Y.Z --draft=false
```

**公開した時点で、全利用者への配布が始まる。** ドラフトは `releases/latest` にならないので、公開するまでセルフアップデータの確認先（`releases/latest/download/latest.json`）は前の版を返し続ける。

**Immutable releases（リポジトリ設定。有効にしてある）により、公開した時点で成果物とタグが不変になる。** 公開後に `latest.json` の欠けや attestation の漏れに気付いても、成果物の差し替えもタグの打ち直しもできず、次のバージョンを出すしかない（消したリリースのタグ名も再利用できない）。**手順 6 の確認を済ませてから公開する。**

## 8. 公開後の確認

```bash
gh release list --limit 1 --json tagName,isDraft,isLatest
gh release verify vX.Y.Z --repo kan/musql
```

先頭が今回のタグで、`isDraft` が `false`、`isLatest` が `true` になっていること。`gh release verify`（公開した時点で GitHub が付ける release attestation）が通ること。通らなければ、Immutable releases が有効のままかを見て（`gh api repos/kan/musql/immutable-releases -q .enabled`）、結果を添えてユーザーに報告する。公開済みのリリースは不変なので直せない。設定を有効に戻すかどうかもユーザーが決める。

あわせて、リリースした版で閉じる issue が残っていないかを見る。

Store 版の提出（Partner Center への MSIX のアップロード）は、このスキルの範囲外。ユーザーが行う。

## タグを打ち直す場合

公開前に限る。

1. タグを消す: `git push origin :refs/tags/vX.Y.Z && git tag -d vX.Y.Z`
2. 直す PR を main にマージする（CHANGELOG を直すなら、main ではもう `## [X.Y.Z] - 日付` になっている節を直す）
3. 直した main の先頭に **annotated の**タグを打って push し、Release を手で起動する（タグの push では起動しない）:

   ```bash
   git switch main && git pull --ff-only
   git tag -a -m "Release vX.Y.Z" vX.Y.Z
   git push origin vX.Y.Z
   gh workflow run release.yml --ref vX.Y.Z
   ```

前の実行のドラフトは消さなくてよい（`draft` ジョブが同じタグのドラフトを消してから作り直す）。タグを消したあいだに main が進むと、その tagpr の run は「タグより新しい版の節がある」で赤になる（手順 2 の PR のマージでもそうなる）。タグを打ち直したら次の run から通るので、放っておいてよい。手順 2 の run が残した版だけ上がったリリース PR（draft）は、マージしない。手順 3 の後に main への push で走る成功した run が、コメントを付けて閉じる。
