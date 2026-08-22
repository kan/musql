# Rust 実装ルール（`src-tauri/`）

Rust 側を触るときに読む。`src-tauri/src/main.rs` に全ロジックを集約し、Docker 連携だけ `src-tauri/src/docker/`（`discovery.rs`, `tunnel.rs`）、1Password 連携は `src-tauri/src/onepassword.rs` に分けてある。

## 秘密情報
- **パスワード / API キー**: `keyring`（Windows Credential Manager）で保存。各パスワード（MySQL / SSH パスフレーズ / SSH パスワード）は `save_*` フラグで keyring 保存 or 都度入力を選択可能。都度入力の場合、query 画面で接続前にモーダルプロンプトを表示。
- **1Password 連携（#80）**: `op` CLI（公式 Rust SDK が無いためサブプロセス起動）を**パスワードの初期取得・更新元**として使う。`src-tauri/src/onepassword.rs`。解決順は「プロンプト入力 → keyring → 1Password」で、op から取れた値は即 keyring に書き戻すため **op が呼ばれるのはマシン毎に 1 回**。接続毎に呼ばない設計にしているのは、Windows では呼び出し元プロセス単位で認証が要る＝毎回 Windows Hello が出るため。参照は `op://vault/item/field` 形式の文字列 1 本（`MySqlConfig.op_ref` / `SshConfig.op_passphrase_ref` / `op_password_ref`）で、秘密ではないので**同期対象**。AI の API キーは同期対象外なので参照を localStorage（`musql:ai:op-ref:<provider>`）に置き `ai_assist` の引数で渡す。コマンド: `op_available`（UI の出し分け）/ `op_read_secret`（設定画面の `1P` ボタン）/ `op_list_items`・`op_list_fields`（参照ピッカー `ui/op-picker.js`、settings と query の両ウィンドウで共有。`window.openOpPicker()` が `Promise<string|null>` を返す）。**`op item get` の応答には `value`（秘密値そのもの）が入るため、`OpField` / `RawField` に `value` フィールドを足してはいけない**（serde が未知フィールドを捨てることで WebView に渡らないことを保証している。`parse_fields_drops_secret_values` テストで固定）。アイテム一覧は `ITEM_CACHE` でセッション内キャッシュ（取得が数秒〜かかるため）。op 呼び出しは `spawn_blocking` + 90 秒タイムアウト、`CREATE_NO_WINDOW` でコンソール抑止。実 CLI を叩くテストは `#[ignore]` 付き（`cargo test -- --ignored`）。

## 接続
- **接続プール**: `ConnectionCache`（`Arc<Mutex>`）で Pool + SshTunnel をキャッシュ。fingerprint 一致で再利用。
- **クエリキャンセル**: `RUNNING_QUERIES` でタブ単位に `KILL QUERY`。
- **SSH**: `russh` v0.57。認証方式は公開鍵（`key`）またはパスワード（`password`）を選択可能。公開鍵: 指定鍵 → agent → デフォルト鍵。パスワード: `session.authenticate_password()`。タイムアウト 8 秒。ssh config 参照時は認証方式を公開鍵に固定。

## Docker 連携
- **Docker 連携**: `bollard` クレートで Docker API に接続（名前付きパイプ → TCP `127.0.0.1:2375/2376` フォールバック、WSL2 dockerd 対応）。running コンテナから MySQL コンテナを自動検出（exposed port 3306 or `musql.enable=true` ラベル）。ports バインドなしのコンテナには `alpine/socat` 一時コンテナで TCP トンネルを作成（`auto_remove: true`、ラベル `musql.tunnel=true`）。トンネルコンテナは検出一覧から除外。アプリ起動時・終了時・query ウィンドウ close 時にトンネルをクリーンアップ。資格情報はコンテナ毎に保持（user/ssl_mode は localStorage、パスワードは keyring）。Cargo feature `docker`（デフォルト有効）で `bollard`/`futures-util` 依存を分離。
- **Docker のラベルカスタマイズ**: `musql.name`（表示名）/ `musql.user` / `musql.password` / `musql.port` をコンテナのラベルに付けると、検出結果と資格情報の初期値に反映される。

## ウィンドウとメニュー
- **メニュー**: ハンバーガーボタン → `popup_menu()`。アクセラレータは非表示メニューバーで保持。
- **ウィンドウ状態永続化**: `tauri-plugin-window-state`（常時有効）で main / query のサイズ・位置・最大化を保存/復元。`StateFlags` は `SIZE | POSITION | MAXIMIZED` のみ（VISIBLE 等は除外＝show/hide パターンを上書きしない）。固定サイズの settings は `with_denylist(["settings"])` で除外。プラグインは `RunEvent::Exit` で自動保存するが、`main:exit` メニューは `std::process::exit(0)` で終了しフックを飛ばすため、その直前で `app.save_window_state(StateFlags::all())` を明示呼び出ししている（この save を消すと状態が保存されない）。dev 起動は `tauri.dev.conf.json` で identifier を `.debug` に分離しているため、インストール版とウィンドウ状態は混ざらない。
- **ウィンドウ管理**: show/hide パターン採用。main 以外のウィンドウ（query, settings）は `on_window_event` で `CloseRequested` を `api.prevent_close()` + `window.hide()` に差し替え、ウィンドウ破棄を防止。query 閉じ時は `query:reset` イベントを emit してから main を show。main ウィンドウの close はそのままアプリ終了。多重起動は意図的に許容している（1 インスタンスで同時に開ける connection は 1 つのため、複数接続は複数インスタンスで行う設計）。`tauri-plugin-single-instance` は導入しないこと。参考知見（[zenn.dev/sttk3/articles/69cb3bd6331325](https://zenn.dev/sttk3/articles/69cb3bd6331325)）:
  - `close()` はウィンドウを完全破棄、`hide()` はメモリ保持で非表示、`destroy()` はイベント発火なしで破棄。macOS では最後のウィンドウを `close()` するとアプリ自動終了するため main は `hide()` が安全（musql は Windows 専用だが将来のクロスプラットフォーム対応時に重要）。
  - macOS 11 で JS 側から `close()` を呼ぶとクラッシュする報告あり → close 処理は Rust 側で実行するのが安全。
  - `is_focused()` は Windows でウィンドウ生成直後にフォーカス判定が遅延する → 複数ウィンドウのアクティブ状態を正確に追跡するには `AppState` で `Focused` イベントをキャッチして明示管理する方が信頼性が高い。
  - close イベントの `listen` で close を再呼出しすると無限ループになる → `once` で1回限りのリスナーにするか、`prevent_close()` + `hide()` パターン（musql 現行方式）で回避。
  - フロントエンドでイベントリスナーを登録した場合、unmount 時に `unlisten` を必ず呼ぶこと（メモリリーク防止）。

  参考知見（同一スタックの pike での運用実績より）:
  - 開発版とインストール版を共存させるには、dev 用の設定ファイル（pike は `tauri.dev.conf.json`）で identifier を別（例: `.debug` サフィックス）に上書きする。window-state 系プラグイン等は identifier 単位で状態を保存するため、identifier が同一のままだと開発版とインストール版で状態が混ざる。導入時はセットで検討する。
  - 素の `listen()`（`@tauri-apps/api/event`）はデフォルト target が `Any` のため、Rust が `emit_to(label, …)` で特定ウィンドウ宛てに送ったイベントでも全ウィンドウで発火する。特定ウィンドウだけで処理したいイベントは `getCurrentWindow().listen()`（target = 自ラベル）で受けること（pike で全ウィンドウが同一ファイルを開こうとする不具合の原因になった）。全ウィンドウにブロードキャストして ID でフィルタする方式とは使い分ける。


## 設定の同期とアップデート
- **接続プロファイルの外部同期（#47）**: 指定パス（Dropbox 等）の JSON にストアをミラーし複数マシン共有。同期パスは AppConfig `sync_path`（環境依存＝非同期）。**書き出しは `save_profiles` 内で自動ミラー**（全ウィンドウの変更を捕捉、同期的なので focus 取り込みと競合しない）。書き出しはシークレット除去（`sanitized_store_json`。パスワード/SSH 秘密は `skip_serializing` で元々出ないが二重防御）。**取り込み** `sync_import`（`refreshProfiles` が起動時/フォーカス時/`profiles:changed` で呼ぶ）は id ベース merge（既存はファイル優先・ローカル固有は保持・削除は非伝播）、読み取り/parse 失敗はローカルへフォールバック、差分時のみ保存、async+spawn_blocking（クラウド placeholder のフリーズ回避）。UI は main メニュー `main:sync-settings` → `openSyncModal`。競合方針は実質セッション単位 last-write-wins。コマンド: `get_sync_path`/`set_sync_path`/`sync_import`/`sync_export`/`pick_sync_path`。
- **アップデート**: `tauri-plugin-updater` + Ed25519 署名。起動3秒後に自動チェック + Help メニューから手動チェック。Cargo feature `self-updater`（デフォルト有効）で分離。`--no-default-features` で Store ビルド（アップデータ無効）。
