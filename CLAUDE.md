# muSQL project notes (for agents)

Windows向け MySQL クライアント。Tauri v2 + Rust backend + 静的 UI（`ui/` の HTML/JS/CSS、Node.js 不要）。SSH 踏み台対応（`russh`）。

## 詳細ルール（毎セッション自動で読み込まれる）

このファイルには全体像と運用だけを置き、領域別の実装メモは `.claude/rules/` に分けてある。**3 つとも起動時に自動で読み込まれる**（`.claude/rules/` の、frontmatter に `paths` を持たないファイルは無条件で読み込まれる）。改めて Read する必要は無い。

- `.claude/rules/rust.md` … Rust 実装ルール（秘密情報の扱い・接続とキャンセル・Docker 連携・ウィンドウ管理・設定同期）
- `.claude/rules/frontend.md` … UI 実装ルール（画面構成・機能ごとの実装メモ・localStorage keys・Limits）
- `.claude/rules/testing.md` … テスト方針（何を自動テストで担保し、何を手動確認に寄せるか）

**ルールファイルに frontmatter の `paths` を付けないこと**（#118 で検討して見送った）。付けると、対象のファイルに Read / Write / Edit ツールで触れたときにしか読み込まれなくなる。Bash（`cat` / `sed -n` / `grep`）や検索で中身を見ただけでは読み込まれないので、「ログに書いてはいけないもの」のような制約が効かないまま作業が進みうる。ファイルが 3 つしか無いうちは、絞る利点のほうが小さい。

## Docs の役割分担
- `README.md` = 人間向け（概要・ダウンロード・機能ハイライト）。`docs/manual/` = エンドユーザーマニュアル（機能別・日本語）。このファイル = 開発/エージェント向け内部情報。重複を避け、各々の役割に閉じる。
- **ユーザー向け挙動を変えたら `docs/manual/` の該当ページも更新する**（画像は `docs/manual/img/`、撮影リストは `docs/manual/img/README.md`。一部の画像は自動で撮れる：`.claude/rules/testing.md` の「マニュアルのスクリーンショット」）。マニュアルは pike 構成に倣い、見出しアンカーは GitHub slug。校正は下の「ドキュメント校正ルール」に従う。

### ドキュメント校正ルール

**日本語のユーザー向けドキュメントを更新・追加したら、コミット前に校正する。**

- 対象：`README.md` / `docs/manual/` 配下 / `CHANGELOG.md`（各 PR が `## [Unreleased]` に足す項目）
- 対象外：`CLAUDE.md`（読み手が開発者の密な技術メモ）、英語で書く `SECURITY.md` と `PRIVACY.md`

1. **textlint（機械チェック）** を `just lint-docs` で回し、今回書いた箇所の指摘を 0 にする（既存の指摘は 4 を参照）。実体は npx なのでリポジトリに textlint は入れない。対象ファイルとルールプリセットは justfile の `lint-docs` レシピを正とする。

2. **`japanese-tech-writing` スキル**（判断ベース）で、textlint が拾えない空句・冗長・演出・論証を点検する。

3. **守る表記規約**:
   - 箇条書きの太字ラベルの区切りは**全角コロン**で `**用語**：説明` と書く。半角コロン `:` は `no-ai-list-formatting` に触れるため使わない
   - 地の文・見出しで **em ダッシュ `—` を使わない**（全角コロンか句読点にする）
   - 誇張語（「大幅に」等）・LLM 空句（「重要なのは」「正面から」「多角的」等）を使わない
   - 二重助詞・一文内の過多カンマ（4 個以上）を避ける

4. **据え置いてよい指摘**:
   - `no-mix-dearu-desumasu`（本文の「です・ます」と箇条書き・表セルの体言止めの混在）と、列挙が主因の `sentence-length`。マニュアルとして自然なので無理に潰さない
   - **CHANGELOG の過去セクション**。出荷済みの記録なので、表記の一括正規化以外は書き換えない。校正するのは `## [Unreleased]` に足す項目だけ
   - 誤検出の常連が 2 つある。UI 名やエスケープシーケンスに出るリテラルの `?`（`no-exclamation-question-mark`）と、行を折り返した括弧（`no-unmatched-pair` が閉じ括弧を見失う）

5. 見出しを変えたら、ページ内アンカー（`](#...)`）と `data-manual` 属性のアンカーとの整合を確認する（「アプリ内マニュアル」参照）。`just check-docs` が機械的に照合する（次節）。

### ドキュメント乖離のチェック

`just check-docs`（= `src-tauri/src/docs_consistency.rs`。`just test` と CI の `check` ジョブにも含まれる）は、機械的に照合できる乖離だけを見る。落ちたらコミット前に直す。

1. このファイルと `.claude/rules/` が挙げるパスが実在するか（削除・改名の取り残し）
2. このファイルと `.claude/rules/` がバッククォートで挙げるシンボル名が、リポジトリのソースに実在するか（関数の改名・削除の取り残し。vendor の `ui/lib/` は出典に数えない）。他所の API の名前は `EXTERNAL_NAMES`、「もう無い」と説明するために出す名前は `GONE_NAMES` に足す（どちらもテストの中）
3. `data-manual` と `createHelpButton` のアンカーが、マニュアルのページと見出しに解決するか
4. マニュアルが参照する画像が実在するか、参照されない画像が `docs/manual/img/` に残っていないか
5. README とマニュアルの間のリンクとページ内アンカーが解決するか

「説明が実装と合っているか」は判定できない。バージョン番号や数値の食い違いも拾わない。そこは差分の性質から自分で判断する。

## How to run
- 開発タスクの入口は `justfile`。`just` でレシピ一覧、`just dev` で起動、`just check` でコミット前チェック一式（fmt / clippy / test / UI lint）。版はリリース PR が上げるので、`just bump` は手で回さない（下の「リリース PR（tagpr）」）。`ci.yml` の各ステップも同じレシピを呼ぶ（fmt / clippy / test をステップごとに分けて呼ぶので、失敗箇所はステップ単位で分かる）。
  - just は cargo の薄いファサードで、cargo から直接叩いても同じ。`cargo dev` の alias（`.cargo/config.toml`）でも起動できる。
  - justfile 先頭の `set windows-shell` は必須。just の Windows 既定シェル `sh -c` は PATH に無く、PATH 上の `bash` は WSL ランチャ（`C:\Windows\System32\bash.exe`）で Windows 側の cargo / tauri が見えないため、Git Bash を明示している。Git を別の場所に入れている環境は `just --shell <bash へのパス>` で上書きする。
  - `release.yml` は just を経由しない。ビルドは `tauri-action` と PowerShell スクリプトが主体で、レシピに寄せても重複が減らないため。
  - リリースまわりのスクリプト（`scripts/*.mjs`。版の更新、CHANGELOG の見出し、ライセンス一覧、リリースノート）は Node.js で書いてある（#149）。「Node.js 不要」はアプリと UI の方針で、これらは CI と開発者の手元でだけ動く。npm の依存は持たない（標準のモジュールだけを使う）。
  - CHANGELOG の読み書き（リリース PR の見出しとリンク、リリースノートの抜き出し）は、lily などと共有する [kan/tagpr-scripts](https://github.com/kan/tagpr-scripts) を subtree で `scripts/tagpr/` に取り込んで使う（#152）。`scripts/release-prepare.mjs` と `scripts/release-notes.mjs` は、版とライセンス一覧の処理を足したラッパー。**`scripts/tagpr/` の中は編集しない**（kan/tagpr-scripts を直し、`git subtree pull -S --prefix=scripts/tagpr https://github.com/kan/tagpr-scripts.git main --squash` で取り込む。使い方は `scripts/tagpr/README.md`）。Biome の対象からも外してある（`biome.json`）。
- `cargo dev`（= `just dev`）で起動（リポジトリ直下 / src-tauri のどちらからでも可。`.cargo/config.toml` の alias で `cargo tauri dev --config tauri.dev.conf.json` に展開される）。dev config は identifier を `...musql.debug` に上書きし、インストール版とウィンドウ状態（`tauri-plugin-window-state`）・アプリデータを分離する。
- `cargo check` / `cargo test` / `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings`。clippy で常時当てる追加の lint は `src-tauri/Cargo.toml` の `[lints.clippy]`（文字列まわり。`&str` には `.to_string()` ではなく `.to_owned()`、`push_str(&format!(…))` ではなく `write!`）、整形の設定は `src-tauri/rustfmt.toml`。
- UI (`ui/*.js`) と `scripts/*.mjs` の lint: `just lint-ui`（= `npx @biomejs/biome@2.4.10 lint --error-on-warnings`。Biome、package.json 不要。設定は `biome.json`、`ui/lib/**` の vendor は除外）。CI は `setup-biome` が入れた `biome` を使うため、レシピは PATH にあればそちらを優先し、無ければ npx にフォールバックする。

## Architecture
- Rust のロジックの中心は `src-tauri/src/main.rs`。Docker 連携、1Password 連携、ログ、通知、仮想デスクトップ、Redis と SQLite と PostgreSQL への接続は別のファイルに分けてある（一覧は `.claude/rules/rust.md` の冒頭）。
- UI は `ui/` の 4 画面（main = プロファイル一覧 / settings = プロファイル編集 / query = DB エクスプローラ / redis = Redis の画面）。ウィンドウは `tauri.conf.json` で事前定義し、show/hide パターンで管理する。
- Cargo feature で機能を分離：`self-updater`（デフォルト有効、Store ビルドでは無効）、`docker`（デフォルト有効、`bollard` / `futures-util` 依存を分離）。

## Build variants
- **Standalone（デフォルト）**: `cargo tauri build` — NSIS インストーラ + セルフアップデータ付き。
- **Store**: `cargo tauri build --config tauri.store.conf.json -- --no-default-features --features docker` — アップデータ無効・Docker有効。Store 用 EXE を生成。
- **Store dev 確認**: `cargo tauri dev --config src-tauri/tauri.store.conf.json -- --no-default-features --features docker`（要 Developer Command Prompt / RC.EXE in PATH）。
- Store 用アイコン: `src-tauri/icons/Square44x44Logo.png`, `Square150x150Logo.png`, `StoreLogo.png`。
- CI: `release.yml` の `build-store` ジョブが Store EXE と MSIX をビルドし（`build` と並列。Tauri CLI は npm のビルド済みのもので、両ジョブとも `release.yml` 先頭の `TAURI_CLI` の版を使う。dependabot は上げないので、`tauri` クレートを上げたら手で合わせる。`screenshots.yml` の `TAURI_CLI` も同じ版にそろえる）、artifact で渡された `publish-store` ジョブが、ドラフトの Release にアップロードする（#138）。ドラフトを作るのは先に走る `draft` ジョブだけにしておく（#149。`build` は `releaseId` で上げ、`build-store` には Release を触らせない）。リリースのジョブは rust-cache を使わない（毎回新しいタグで走るので復元されず、保存の時間だけがかかる）。MSIX は `store/AppxManifest.xml` + `store/build-msix.ps1` で生成する（未署名。Microsoft 側で署名される）。
- Artifact attestations（#130）: `release.yml` の `build` と `build-store` が、手でダウンロードする 4 つ（NSIS の `-setup.exe`、`.msi`、`muSQL-store-x64.exe`、`muSQL-store-x64.msix`）に `actions/attest` で来歴の証明を付ける。`.sig` と `latest.json` は対象外（updater の経路は minisign の検証で足りている）。グロブは拡張子の末尾まで書く（`bundle/` には配布しない `.msi.zip` / `.nsis.zip` と `.sig` も並ぶ）。attest は成果物への最後の書き換えのあとに置く（`build-store` では MSIX の生成のあと）。グロブの取りこぼしは CI では分からないので、公開前に 4 つとも検証する手順が `release` スキルにある。SLSA Build L3（ビルドを reusable workflow に切り出し、署名用の secrets を持つジョブから分ける）は狙っていない。
- Partner Center Identity: `58967CommunitylinksInc.muSQL` / `CN=46BBEF28-6777-4EF5-AD2C-F9AD9123AA82`。

## action の固定（workflow lockfile、#139）
- **action は `.github/workflows/actions.lock` でコミットに固定する**（GitHub Actions の workflow lockfile。2026-10-28 に GA。pike と同じ運用）。生成と更新は GitHub CLI の拡張 `gh actions-lock`（`gh extension install github/gh-actions-lock`）で行い、**手で編集しない**。推移的な依存（composite action が呼ぶ action。今は `extractions/setup-just` の中の `extractions/setup-crate` と、`actions-rust-lang/audit` の中の `actions/cache`）と、参照先リポジトリの ID も記録される。`github/codeql-action/init` と `/analyze` のようなサブパス付きの参照は、リポジトリ単位の 1 エントリになる。
- **`uses:` には SHA を書かず、タグかブランチで書く**（`actions/checkout@v7.0.1`）。SHA を直接書くと lockfile に入らず、生成そのものが失敗する（解決できない参照が 1 つでもあると何も書き込まれない）。
- **`uses:` を足す・消す・ワークフローを足したら `gh actions-lock` を流し直し、lockfile も同じコミットに入れる。** 載っていない参照があると、そのワークフローはジョブが始まる前に startup_failure で止まる。コミット前の確認は `gh actions-lock --verify`（書き込まずに検査する。ネットワークに出る）。CI でも `verify-lockfile.yml` が PR と main への push のたびに同じ検査を回す（#152。拡張の版は `--pin` で固定してあり、Dependabot は上げない。lockfile の形式が上がって落ちたら、手元の拡張と同じ版に上げる）。
- **タグの更新は Dependabot が lockfile ごと上げる**（github-actions の更新 PR が対応するエントリも作り直す）。`@v4` のような部分版は生成時に完全な版（`@v4.0.0`）へ書き換わる。
- **`dtolnay/rust-toolchain` は `@master` で参照する**（ツールチェーンは `toolchain: stable` の入力で選ぶ）。この action の `stable` などのブランチは上流で作り直されるため、固定した SHA がどのブランチにもタグにも無くなり、lockfile に拒否される（pike で起きた）。ブランチ参照は Dependabot が追わないので、上げるときは `gh actions-lock --relock` で全参照を解決し直し、`commit` の差分を確かめてからコミットする。

## コミット前チェック

**コミットの前は、変更の種類に応じて次を実行し、指摘を反映してからコミットする。**

| 変更の種類 | 実行するもの |
|---|---|
| コードの変更 | ユーザーレベルの `rules/quality-check.md` に従ったレビュー → `just check`（規模の判定、`/simplify` と `/code-review` の順序、effort の指定は `rules/quality-check.md` を正とする） |
| ドキュメントのみ（`README.md` / `docs/manual/` / `CHANGELOG.md`） | 「ドキュメント校正ルール」の校正 |

- レビューで生じた書き換え（`/simplify` の適用と `/code-review` の指摘の反映）は、必ず**ユーザーの動作確認より前**に済ませる（ユーザーは適用後のコードを試す）
- `/code-review` はユーザーがコマンドを打つこともある

## Git workflow

**変更は issue ごとにブランチを切り、PR を作って squash でマージする**（#149）。**この repo はアカウント共通のルールにある「GitHub の個人の repo は PR 運用なし」の例外**で（ルートに `.tagpr` がある）、こちらを優先する。**main へ直接コミットも push もしない。** エージェントが変更を加えるときは次の流れを守る。

1. 対象の issue を確かめ、最新の main から `feature/<issue 番号>-<短い名前>`（バグ修正は `fix/`）のブランチを切る。issue の無い変更は、先に issue を立てるかをユーザーに確認する
2. 利用者に届く変更は、同じブランチで `CHANGELOG.md` の `## [Unreleased]` に項目を足す（下の「CHANGELOG の書き方」）
3. **コミットの前にユーザーの動作確認 OK を取る。** lint / test が通っただけでコミットしない（GUI アプリなので実際に触らないと分からない）
4. コミットの承認を得たら、コミット → ブランチの push → `gh pr create` までを続けて行う（承認はこの 3 つをまとめて指す）
   - 本文に `Closes #<番号>` を書く
   - **ラベルは issue に合わせて `enhancement`（機能）か `bug`（修正）を付ける。** tagpr は `enhancement` の付いた PR があれば次の版を minor に、無ければ patch にする
   - **タイトルは squash したコミットの件名になる**ので、今までのコミットと同じ日本語の 1 行で書く（GitHub が末尾に `(#PR 番号)` を足す）
5. **Copilot のレビューを待ち、指摘を 1 件ずつ採否を決める**（main 向けの PR に自動で付く。ルールセット「copilot review」。draft と push ごとの再レビューは対象外）。`/code-review` と同じ扱いで、一括では反映しない
   - 指摘は `gh api repos/kan/musql/pulls/<番号>/comments`（行への指摘）と `gh pr view <番号> --comments`（全体のコメント）で読む
   - 採った指摘はそのブランチで直して push する。直したら `just check` を通し直す。**挙動が変わる修正なら、ユーザーの動作確認を取り直す**
   - 採らなかった指摘には、理由をその指摘への返信で書く（`gh api -X POST repos/kan/musql/pulls/<番号>/comments/<id>/replies -f body=...`）。ユーザーへの報告にも理由を添えて並べる
6. CI が通り、Copilot の指摘を片付けたら、**ユーザーの承認を得てから** `gh pr merge <番号> --squash --delete-branch` でマージする（コミットの承認とは別に聞く）。マージしたら手元の main を `git pull --ff-only` で進める

- **例外：subtree の取り込み。** `scripts/tagpr/` を `git subtree add` / `pull` する PR は、**squash せずマージコミットでマージする**（`gh pr merge <番号> --merge --delete-branch`）。`git subtree` は前回の取り込みを自分が作ったコミットから探すので、squash すると次の pull が壊れる。取り込みは `-S` を付けて署名する（`git subtree` は `commit.gpgsign` を見ない）。取り込みは CHANGELOG に書かない。`git subtree` はその場でコミットを作るので、コミットの承認を得てから実行する
- **dependabot の PR** も PR のまま squash でマージする。cargo の PR は同じ `Cargo.lock` を触るので、1 件マージするたびに残りが作り直しになる（`.github/dependabot.yml` の `groups` で ecosystem ごとに 1 つにまとめてある）。**dependabot のブランチへ手で push しない**（以後 dependabot が rebase しなくなる）。直しが要るときは、マージしてから直す PR を作る。CHANGELOG の行はその PR か、リリースの前の PR で足す
- **例外：リリース依頼。** 「リリースして」は、リリース PR の確認からマージ、ドラフトの公開までの一括の依頼。リリース PR のマージに個別の確認は要らない。`release` スキルの手順を最後まで実行する

## CHANGELOG の書き方

- **各 PR が `## [Unreleased]` の節に自分の項目を足す。** 節はいちばん上の版の節の上に常にある（リリース PR が版の節を作るたびに、空の節を戻す）。中の小見出し（`### Added` / `### Changed` / `### Fixed` / `### Security`）と項目の書式は過去の節に合わせ、末尾近くに issue 番号を `（#148）` の形で入れる
- `## [Unreleased]` は常に 1 つ。**版と日付、末尾のリンクは書かない**。リリース PR で `scripts/release-prepare.mjs` が `## [X.Y.Z] - 日付` に書き換え、空の `## [Unreleased]` を上に戻し、末尾のリンク定義の先頭に `[X.Y.Z]: https://github.com/kan/musql/releases/tag/vX.Y.Z` を足す（`[Unreleased]` のリンクは置かない）。その節がリリースノートと `latest.json` の `notes` になる
- 節は、次の `## ` の見出しか、ファイル末尾のリンク定義のまとまりで終わる。項目の中のリンクはインラインで書く（参照リンクの定義は節の外にあるので、リリースノートではリンクにならない）。リリースノートでは、段落と項目の中の折り返しが 1 行につながる（日本語どうしと全角の句読点・括弧の前後は空白なし）。改行は LF
- 利用者に届かない変更（CI、開発ノート、テストだけの変更）は書かない
- `## [Unreleased]` が空のまま main が進むと、tagpr の run がリリース PR の準備で失敗する（リリースの直後は毎回そうなる。リリースできない状態の印で、利用者に届く変更の PR が入れば解消する）

## Release procedure

手順は `release` スキル（`.claude/skills/release/SKILL.md`）にある。**リリースを頼まれたら、そのスキルを起動して最後まで実行する。** ここには手順を書かない（毎セッション読み込まれるこのファイルを短く保つため）。

押さえておく前提は次の 2 つ。

- `Release` ワークフローは、Release を**ドラフト**で作る。アセットを確認して公開するまで、利用者（セルフアップデータ）には配られない
- リリースの依頼は、リリース PR のマージから公開までを含む一括の依頼（上の「Git workflow」の例外規定）

### リリース PR（tagpr、#149）

- **リリースは [tagpr](https://github.com/Songmu/tagpr) のリリース PR をマージして始める**（pike と同じ構成）。main に push があるたびに `tagpr.yml` が、前のタグ以降の変更を載せたリリース PR を作り直す。**ブランチ名は `tagpr-from-v<前回の版>`**。マージすると、次の run がマージ後の main の先頭にタグを打ち、同じ run の `release` ジョブが `release.yml` をそのタグを ref にした `workflow_dispatch` で起動する（#152。`workflow_call` で呼ばない。run の ref と attestation の source ref が main になるため）。設定は `.tagpr`
- **リリース PR の中身は `scripts/release-prepare.mjs` がそろえる**（`.tagpr` の `postVersionCommand`）。tagpr が `tauri.conf.json` の版を書き換えた後に呼ばれ、`Cargo.toml` と `Cargo.lock`（`scripts/bump-version.mjs`）、CHANGELOG の見出しとリンク（`scripts/tagpr/release-prepare.mjs --changelog-only`）、ライセンス一覧（`scripts/credits.mjs`）をそろえる。PR を作り直すたびに main から走る。CHANGELOG の段を最初に走らせ、そこで落ちたら（リリースの直後は毎回）残りは走らせない
  - **CHANGELOG に最新のタグより新しい版の節があると失敗する**（`scripts/tagpr/` の検査）。リリース PR はマージされたのにタグが打たれていない印で、多いのはマージの run が後の push で cancelled になったとき。新しいリリース PR をマージせず、マージコミットの tagpr の run を再実行する（`release` スキル）
  - **`versionFile` は `tauri.conf.json` の 1 つだけ。** tagpr はファイルの中の旧版の文字列を最初の 1 箇所だけ置き換えるので、依存の版を巻き込みうる `Cargo.toml` は、1 箇所だけ当たることを確かめる `bump-version.mjs` に任せる。マージ後のタグもこのファイルの版から決まる
  - **`Cargo.lock` はビルドせずに更新する**（`cargo update --workspace`。musql 自身の行だけが変わる）。ランナーは ubuntu で、`cargo check` は WebView の開発パッケージが無いと通らない
  - **コマンドが失敗しても tagpr はリリース PR を作り、ステップを失敗で終える。** tagpr の run が赤ならリリース PR をマージしない
- **版は PR のラベルで決まる。** マージ済みの PR に `enhancement` があれば minor（`.tagpr` の `minorLabels`）、無ければ patch。**版を変えたいときは `tauri.conf.json` を手で書き換えない**（残りのファイルがそろわない）。リリース PR に `tagpr:minor` を付けて `gh workflow run tagpr.yml` で作り直す
- **CHANGELOG と GitHub のリリースは tagpr に作らせない**（`changelog = false` / `release = false`）。tagpr の CHANGELOG は PR タイトルを並べるだけなので、利用者向けの説明は各 PR が書く（上の「CHANGELOG の書き方」）。Immutable releases を有効にしてあるので、tagpr がリリースを公開すると成果物を足せなくなる。ドラフトは `release.yml` の `draft` ジョブが作り、本文は `scripts/release-notes.mjs` が CHANGELOG の節から作る（同じ本文が `latest.json` の `notes` にも入る。公開後は差し替えられない）。リリース PR の本文に出る PR の一覧は `.github/release.yml` の分類に従う
- **ライセンス一覧はリリース PR が作り直す。** 依存を更新する PR では作り直さず、照合（`just check-credits`）も `just test` と CI から外してある（理由は `scripts/credits.mjs` の冒頭）
- **スクリーンショットはリリース PR のブランチで撮る**（`screenshots.yml` を `--ref tagpr-from-v<前回の版>` で起動）。`commit: true`（既定）なら画像を `docs/manual/img/` へ写し、差分があればそのブランチへコミットする。tagpr はリリース PR を作り直すときもこのコミットを引き継ぐ。撮影中に tagpr がブランチを作り直すと push が fast-forward にならずに落ちる（古いツリーの画像を載せない）。起動し直す
- **トークンは GITHUB_TOKEN のまま**（GitHub App は作っていない）。GITHUB_TOKEN で打ったタグの push ではワークフローが起動しないので、`release.yml` を同じ run から `workflow_dispatch` で起動する（`workflow_dispatch` はこの制限の例外。ジョブには `actions: write` を付ける）。その代わり、**ボットが作ったリリース PR の CI と CodeQL の run は承認待ち（`action_required`）で止まる**（撮影のコミットのたびにもできる）。承認は `gh api -X POST repos/kan/musql/actions/runs/<id>/approve` で通る。tagpr が PR を作るには、リポジトリの設定「Allow GitHub Actions to create and approve pull requests」が要る
- **tagpr のジョブは書き込みの権限（`contents` / `pull-requests`）を持つので、musql と依存のコードを走らせない。** 呼ぶのは `cargo update --workspace` と `cargo metadata` だけで、build.rs は走らない。ビルドやテストをこのジョブに足さない。Rust のツールチェーンも入れない（ランナーの stable を使う）
- **`concurrency` は tagpr のジョブだけに付け、`cancel-in-progress: false`。** tagpr どうしが同じブランチを作り直し合わないように並べず、タグを打った直後に止めて成果物の無いタグを残さないように止めない。`release` ジョブは含めない
- **attestation の署名者は `release.yml`、source の ref はタグ（`refs/tags/vX.Y.Z`）**（#152 から。タグで dispatch するため）。#152 より前の版（v0.11.1 まで）は `workflow_call` で呼んでいたので、source の ref が `refs/heads/main` になっている。それらを検証するときは `--source-ref` にタグを渡さない

## 計画

未着手の計画は GitHub issue で管理する（このファイルには置かない）。`gh issue list --label enhancement` で見る。
