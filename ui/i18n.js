(function () {
  var STORAGE_KEY = "musql:lang";

  var LANG = {
    en: {
      // ── Main window ──
      connections: "Connections",
      new_group: "New group",
      new_profile: "New",
      filter_placeholder: "Search connections...",
      host_not_set: "host not set",
      ctx_open: "Open",
      ctx_settings: "Settings",
      ctx_duplicate: "Duplicate",
      ctx_delete: "Delete",
      ctx_add_setting: "Add profile",
      ctx_rename: "Rename",
      confirm_delete_profile: "Delete this profile?",
      prompt_group_name: "Enter group name:",
      prompt_rename_group: "New group name:",
      confirm_delete_group: "Delete this group?\n(Profiles inside will be moved to root)",
      empty_state_msg: "No connections yet",
      empty_state_btn: "New Connection",

      // ── Settings window ──
      settings_heading: "Settings",
      connection_profile: "Connection profile",
      general: "General",
      profile_name: "Profile name",
      color: "Color",
      color_none: "None",
      tags: "Tags",
      add_tag_placeholder: "Add tag...",
      mysql: "DB Settings",
      db_engine: "DB Engine",
      host: "Host",
      port: "Port",
      database: "Database",
      user: "User",
      password: "Password",
      ssl_mode: "SSL Mode",
      ca_certificate: "CA Certificate",
      ssh_via: "SSH",
      ssh_bastion: "SSH Bastion",
      enable: "Enable",
      config_host: "Config Host",
      config_host_manual: "(manual)",
      ssh_config_load: "Load from ssh config",
      ssh_config_ref: "Referencing ssh config ({name})",
      ssh_config_clear: "Clear",
      ssh_config_select: "Select SSH Config Host",
      identity_file: "IdentityFile",
      ssh_passphrase: "Passphrase",
      ssh_passphrase_saved_placeholder: "(saved - leave blank to keep)",
      browse: "Browse...",
      test_connection: "Test",
      connect: "Connect",
      save: "Save",
      cancel: "Cancel",
      delete: "Delete",
      connecting: "connecting...",
      profile_name_required: "profile name is required",
      password_saved_placeholder: "(saved - leave blank to keep)",
      clear_password: "Clear saved password",
      clear_ssh_passphrase: "Clear saved passphrase",
      password_cleared: "(will be cleared on save)",
      ssh_passphrase_cleared: "(will be cleared on save)",
      select_ca_cert: "Select CA Certificate",
      select_identity_file: "Select Identity File",
      profile_name_placeholder: "e.g. prod/analytics",
      optional: "optional",

      // ── Query window ──
      select_database: "Select Database",
      db_switch_title: "Switch DB",
      add_sql_tab_title: "Add SQL tab",
      loading: "Loading...",
      running: "Running...",
      query_cancelled: "Query cancelled.",
      zero_rows: "0 rows",
      rows_range: "Rows {from}\u2013{to} of {total}",
      n_columns: "{n} columns",
      n_rows: "{n} rows",
      affected_rows: "Affected rows: {n}",
      row_detail: "Row Detail",
      prev: "Prev",
      next: "Next",
      truncate: "Truncate",
      truncate_title: "Truncate BLOB/TEXT columns",
      schema: "Structure",
      schema_suffix: "(schema)",
      data: "Data",
      indexes: "Indexes",
      export: "Export",
      history: "History",
      no_history: "(no history)",
      history_open: "Open in new tab",
      format: "Format",
      run_this_line: "Run this line",
      run_all: "Run all",
      place_cursor_hint: "Place the cursor on the statement you want to execute.",
      csv_current: "CSV (current)",
      tsv_current: "TSV (current)",
      csv_all: "CSV (all rows)",
      tsv_all: "TSV (all rows)",
      sql_all: "SQL (all rows)",

      // ── Import/Export ──
      export_profiles_title: "Export profiles",
      import_profiles_title: "Import profiles",
      confirm_include_passwords: "Include passwords in the export file?",
      import_success: "{n} profile(s) imported.",
      import_conflicts: "Found {groups} duplicate group(s) and {profiles} duplicate profile(s).\nOverwrite existing? (Cancel = add as new)",

      // ── Update ──
      update_available: "Update v{version} available",
      update_install: "Update & Restart",
      update_installing: "Installing...",
      check_update: "Check for Updates...",
      no_update_available: "You are using the latest version.",

      // ── AI ──
      ai_settings: "AI Settings",
      ai_provider: "Provider",
      ai_model: "Model",
      ai_api_key: "API Key",
      ai_requesting: "Requesting...",
      ai_not_configured: "AI is not configured. Please set up the provider and API key.",
      ai_assist: "AI Assist",
      ai_assist_placeholder: "Ask AI to write SQL...",
      ai_assist_send: "Send",
      ai_assist_copy: "Copy",
      ai_assist_insert: "Insert",
      ai_assist_clear: "Clear chat",
      ai_assist_empty: "Ask AI to generate SQL queries based on your database schema. Describe what you need in natural language.",

      // ── Common ──
      toggle_dark_mode: "Toggle dark mode",
      toggle_language: "Switch language",
    },
    ja: {
      // ── メイン画面 ──
      connections: "接続一覧",
      new_group: "新規グループ",
      new_profile: "新規",
      filter_placeholder: "接続先を検索...",
      host_not_set: "ホスト未設定",
      ctx_open: "開く",
      ctx_settings: "設定",
      ctx_duplicate: "複製",
      ctx_delete: "削除",
      ctx_add_setting: "設定を追加",
      ctx_rename: "リネーム",
      confirm_delete_profile: "このプロファイルを削除しますか？",
      prompt_group_name: "グループ名を入力してください:",
      prompt_rename_group: "新しいグループ名:",
      confirm_delete_group: "このグループを削除しますか？\n（中のプロファイルはルートに移動します）",
      empty_state_msg: "接続先がありません",
      empty_state_btn: "新規接続",

      // ── 設定画面 ──
      settings_heading: "設定",
      connection_profile: "接続プロファイル",
      general: "全般",
      profile_name: "プロファイル名",
      color: "色",
      color_none: "なし",
      tags: "タグ",
      add_tag_placeholder: "タグを追加...",
      mysql: "DB設定",
      db_engine: "DBエンジン",
      host: "ホスト",
      port: "ポート",
      database: "データベース名",
      user: "ユーザー",
      password: "パスワード",
      ssl_mode: "SSL モード",
      ca_certificate: "CA 証明書",
      ssh_via: "SSH経由",
      ssh_bastion: "SSH 踏み台",
      enable: "有効",
      config_host: "Config Host",
      config_host_manual: "（手動）",
      ssh_config_load: "ssh configから情報を読み込む",
      ssh_config_ref: "ssh config ({name}) を参照",
      ssh_config_clear: "解除",
      ssh_config_select: "SSH Config Host を選択",
      identity_file: "IdentityFile",
      ssh_passphrase: "パスフレーズ",
      ssh_passphrase_saved_placeholder: "（保存済み - 変更しない場合は空欄）",
      browse: "Browse...",
      test_connection: "接続テスト",
      connect: "接続",
      save: "保存",
      cancel: "キャンセル",
      delete: "削除",
      connecting: "接続中...",
      profile_name_required: "プロファイル名は必須です",
      password_saved_placeholder: "（保存済み - 変更しない場合は空欄）",
      clear_password: "保存済みパスワードを削除",
      clear_ssh_passphrase: "保存済みパスフレーズを削除",
      password_cleared: "（保存時に削除されます）",
      ssh_passphrase_cleared: "（保存時に削除されます）",
      select_ca_cert: "CA 証明書を選択",
      select_identity_file: "Identity ファイルを選択",
      profile_name_placeholder: "例: prod/analytics",
      optional: "任意",

      // ── クエリ画面 ──
      select_database: "データベースを選択",
      db_switch_title: "DB 切替",
      add_sql_tab_title: "SQL タブ追加",
      loading: "読み込み中...",
      running: "実行中...",
      query_cancelled: "クエリがキャンセルされました。",
      zero_rows: "0 件",
      rows_range: "{from}\u2013{to} 件 / {total} 件中",
      n_columns: "{n} カラム",
      n_rows: "{n} 件",
      affected_rows: "影響行数: {n}",
      row_detail: "行の詳細",
      prev: "前へ",
      next: "次へ",
      truncate: "短縮表示",
      truncate_title: "BLOB/TEXT カラムの短縮表示",
      schema: "構造",
      schema_suffix: "（構造）",
      data: "データ",
      indexes: "インデックス",
      export: "ファイル出力",
      history: "履歴",
      no_history: "（履歴なし）",
      history_open: "新しいタブで開く",
      format: "整形",
      run_this_line: "この行を実行",
      run_all: "全て実行",
      place_cursor_hint: "実行するSQLにカーソルを置いてください。",
      csv_current: "CSV（現在の表示）",
      tsv_current: "TSV（現在の表示）",
      csv_all: "CSV（全行）",
      tsv_all: "TSV（全行）",
      sql_all: "SQL（全行）",

      // ── インポート/エクスポート ──
      export_profiles_title: "プロファイルをエクスポート",
      import_profiles_title: "プロファイルをインポート",
      confirm_include_passwords: "エクスポートファイルにパスワードを含めますか？",
      import_success: "{n} 件のプロファイルをインポートしました。",
      import_conflicts: "重複するグループが {groups} 件、プロファイルが {profiles} 件あります。\n既存の設定を上書きしますか？（キャンセル = 新規として追加）",

      // ── アップデート ──
      update_available: "v{version} へのアップデートがあります",
      update_install: "更新して再起動",
      update_installing: "インストール中...",
      check_update: "アップデートを確認...",
      no_update_available: "最新バージョンです。",

      // ── AI ──
      ai_settings: "AI 設定",
      ai_provider: "プロバイダ",
      ai_model: "モデル",
      ai_api_key: "API キー",
      ai_requesting: "リクエスト中...",
      ai_not_configured: "AI が設定されていません。プロバイダと API キーを設定してください。",
      ai_assist: "AI アシスト",
      ai_assist_placeholder: "AIにSQLを書いてもらう...",
      ai_assist_send: "送信",
      ai_assist_copy: "コピー",
      ai_assist_insert: "挿入",
      ai_assist_clear: "チャットをクリア",
      ai_assist_empty: "データベースのスキーマに基づいて、AIがSQLクエリを生成します。必要なことを自然言語で記述してください。",

      // ── 共通 ──
      toggle_dark_mode: "ダークモード切替",
      toggle_language: "言語を切替",
    },
  };

  function detectLang() {
    var saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "en" || saved === "ja") return saved;
    var nav = (navigator.language || "").toLowerCase();
    var detected = nav.startsWith("ja") ? "ja" : "en";
    // Persist detected language so it doesn't re-detect
    localStorage.setItem(STORAGE_KEY, detected);
    return detected;
  }

  var currentLang = detectLang();

  function t(key, params) {
    var dict = LANG[currentLang] || LANG.en;
    var str = dict[key];
    if (str === undefined) {
      // Fallback to English
      str = LANG.en[key];
    }
    if (str === undefined) return key;
    if (params) {
      Object.keys(params).forEach(function (k) {
        str = str.replace(new RegExp("\\{" + k + "\\}", "g"), params[k]);
      });
    }
    return str;
  }

  function getLang() {
    return currentLang;
  }

  function setLang(lang) {
    if (lang !== "en" && lang !== "ja") return;
    currentLang = lang;
    localStorage.setItem(STORAGE_KEY, lang);
    applyI18n();
    window.dispatchEvent(new CustomEvent("musql:langchange"));
  }

  function applyI18n() {
    document.documentElement.lang = currentLang;

    // data-i18n → textContent
    document.querySelectorAll("[data-i18n]").forEach(function (el) {
      el.textContent = t(el.getAttribute("data-i18n"));
    });
    // data-i18n-placeholder → placeholder
    document.querySelectorAll("[data-i18n-placeholder]").forEach(function (el) {
      el.placeholder = t(el.getAttribute("data-i18n-placeholder"));
    });
    // data-i18n-title → title
    document.querySelectorAll("[data-i18n-title]").forEach(function (el) {
      el.title = t(el.getAttribute("data-i18n-title"));
    });
    // data-i18n-html → innerHTML (for elements that mix icon + text)
    document.querySelectorAll("[data-i18n-html]").forEach(function (el) {
      var key = el.getAttribute("data-i18n-html");
      var iconName = el.getAttribute("data-i18n-icon");
      var iconSize = el.getAttribute("data-i18n-icon-size");
      if (iconName && typeof icon === "function") {
        el.innerHTML = icon(iconName, iconSize ? Number(iconSize) : undefined) + " " + t(key);
      } else {
        el.textContent = t(key);
      }
    });
  }

  // Apply immediately
  document.documentElement.lang = currentLang;

  // Apply after DOM ready
  document.addEventListener("DOMContentLoaded", function () {
    applyI18n();
    // Language toggle is now menu-only; no floating button.
  });

  // Sync across windows via storage event
  window.addEventListener("storage", function (e) {
    if (e.key === STORAGE_KEY) {
      var newLang = e.newValue;
      if (newLang === "en" || newLang === "ja") {
        currentLang = newLang;
        applyI18n();
        window.dispatchEvent(new CustomEvent("musql:langchange"));
      }
    }
  });

  // Expose globals
  window.t = t;
  window.getLang = getLang;
  window.setLang = setLang;
  window.applyI18n = applyI18n;
})();
