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

      // ── Settings window ──
      settings_heading: "Settings",
      connection_profile: "Connection profile",
      general: "General",
      profile_name: "Profile name",
      color: "Color",
      color_none: "None",
      tags: "Tags",
      add_tag_placeholder: "Add tag...",
      mysql: "MySQL",
      host: "Host",
      port: "Port",
      database: "Database",
      user: "User",
      password: "Password",
      ssl_mode: "SSL Mode",
      ca_certificate: "CA Certificate",
      ssh_bastion: "SSH Bastion",
      enable: "Enable",
      config_host: "Config Host",
      config_host_manual: "(manual)",
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
      schema: "View schema",
      schema_suffix: "(schema)",
      data: "View data",
      export: "Export",
      history: "History",
      no_history: "(no history)",
      format: "Format",
      run_this_line: "Run this line",
      run_all: "Run all",
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

      // ── 設定画面 ──
      settings_heading: "設定",
      connection_profile: "接続プロファイル",
      general: "全般",
      profile_name: "プロファイル名",
      color: "色",
      color_none: "なし",
      tags: "タグ",
      add_tag_placeholder: "タグを追加...",
      mysql: "MySQL",
      host: "ホスト",
      port: "ポート",
      database: "データベース",
      user: "ユーザー",
      password: "パスワード",
      ssl_mode: "SSL モード",
      ca_certificate: "CA 証明書",
      ssh_bastion: "SSH 踏み台",
      enable: "有効",
      config_host: "Config Host",
      config_host_manual: "（手動）",
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
      truncate: "カラム切詰",
      truncate_title: "BLOB/TEXT カラムの切り詰め表示",
      schema: "構造を参照",
      schema_suffix: "（構造）",
      data: "データ閲覧",
      export: "ファイル出力",
      history: "履歴",
      no_history: "（履歴なし）",
      format: "整形",
      run_this_line: "この行を実行",
      run_all: "全て実行",
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

      // ── 共通 ──
      toggle_dark_mode: "ダークモード切替",
      toggle_language: "言語を切替",
    },
  };

  function detectLang() {
    var saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "en" || saved === "ja") return saved;
    var nav = (navigator.language || "").toLowerCase();
    if (nav.startsWith("ja")) return "ja";
    return "en";
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

    // Language toggle button (main window only)
    if (document.body.hasAttribute("data-theme-toggle")) {
      var btn = document.createElement("button");
      btn.className = "lang-toggle";
      btn.title = t("toggle_language");

      function updateLabel() {
        btn.textContent = currentLang === "ja" ? "EN" : "JA";
        btn.title = t("toggle_language");
      }

      btn.addEventListener("click", function () {
        setLang(currentLang === "ja" ? "en" : "ja");
        updateLabel();
      });

      updateLabel();
      document.body.appendChild(btn);
    }
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
