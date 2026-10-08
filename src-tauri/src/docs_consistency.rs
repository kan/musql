//! Mechanical checks that the docs still match the code (#115).
//!
//! Nothing here judges whether a description is *right*. It only catches the drift a
//! person overlooks: a renamed function still named in the notes, a help button whose
//! anchor no longer exists, an image nobody references.
//!
//! 1. Paths named in the dev notes (CLAUDE.md, .claude/rules) exist.
//! 2. Symbols named in the dev notes exist somewhere in the repository's own sources.
//! 3. Every in-app help target (`data-manual`, `createHelpButton`) resolves to a manual
//!    page and heading.
//! 4. Manual images exist, and no image is left unreferenced.
//! 5. Links and anchors between README and the manual pages resolve.
//!
//! Rust tests rather than a script because the project does not require Node.js. They
//! are compiled only under `cfg(test)` and run with `just test` and in the CI `check`
//! job; `just check-docs` runs them alone.
//!
//! **A unit-test module on purpose, not `src-tauri/tests/`.** An integration test makes
//! cargo build the real `musql.exe` as well, and on Windows that file is locked while
//! `just dev` is running, so the check would fail exactly when someone is working.
//!
//! **"Exists" always means "is in the repository"** (tracked, or new and not ignored),
//! asked of git rather than of the disk. A generated or ignored file would otherwise
//! make a check pass on one machine and fail on CI.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Allow lists ──────────────────────────────────────────────────────────────

/// Names that belong to something else (a dependency, the OS, GitHub) and are named in
/// the notes on purpose. They are *supposed* to be absent from muSQL's own code.
const EXTERNAL_NAMES: &[&str] = &[
    // A general Tauri pattern from the article rust.md cites (tracking focus in app
    // state). muSQL has no such type.
    "AppState",
    // This module's own identifiers. It is left out of the corpus (its allow lists and
    // tests name things that do not exist), so the notes that describe it need these.
    "EXTERNAL_NAMES",
    "GONE_NAMES",
    "heading_slugs",
];

/// Names the notes mention to say they do not (or must not) exist. When removing one,
/// reread the sentence that mentions it.
const GONE_NAMES: &[&str] = &[
    // The tauri-action option before v1.0.0 renamed it to `uploadUpdaterJson`. CLAUDE.md
    // names it to explain why the setting is easy to get wrong.
    "includeUpdaterJson",
];

// ── The repository ───────────────────────────────────────────────────────────

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a parent")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
}

/// Every file in the repository, as forward-slash relative paths: tracked files plus new
/// ones that are not ignored (so a file added in the same change counts before commit).
fn repo_files() -> Vec<String> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root())
        .output()
        .expect("git ls-files runs");
    assert!(output.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&output.stdout)
        .split('\0')
        // The index still lists a file removed with plain `rm`. Drop those, or a
        // leftover reference to it would only be caught after the commit, on CI.
        .filter(|f| !f.is_empty() && root().join(f).exists())
        .map(str::to_owned)
        .collect()
}

/// `files` plus every directory that contains one of them.
fn repo_paths(files: &[String]) -> HashSet<String> {
    let mut paths = HashSet::new();
    for file in files.iter().cloned() {
        let mut end = file.len();
        while let Some(slash) = file[..end].rfind('/') {
            paths.insert(file[..slash].to_string());
            end = slash;
        }
        paths.insert(file);
    }
    paths
}

/// Repository files directly under `dir` whose name ends with one of `exts`, sorted.
fn files_in(files: &[String], dir: &str, exts: &[&str]) -> Vec<String> {
    let prefix = format!("{dir}/");
    let mut out: Vec<String> = files
        .iter()
        .filter(|f| {
            f.strip_prefix(&prefix)
                .is_some_and(|name| !name.contains('/') && exts.iter().any(|e| name.ends_with(e)))
        })
        .cloned()
        .collect();
    out.sort();
    out
}

/// CLAUDE.md and the rule files: the notes whose names are meant to point at real code.
fn note_files(files: &[String]) -> Vec<String> {
    let mut notes = vec!["CLAUDE.md".to_owned()];
    notes.extend(files_in(files, ".claude/rules", &[".md"]));
    notes
}

/// README and the manual pages: what a user reads.
fn doc_files(files: &[String]) -> Vec<String> {
    let mut docs = vec!["README.md".to_owned()];
    docs.extend(files_in(files, "docs/manual", &[".md"]));
    docs
}

// ── Markdown, read just far enough ───────────────────────────────────────────

/// Drops fenced code blocks. Markdown inside one is notation being shown, not markup.
fn strip_fences(body: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The contents of single-backtick spans, line by line.
fn code_spans(body: &str) -> Vec<&str> {
    body.lines()
        .flat_map(|line| line.split('`').skip(1).step_by(2))
        .filter(|span| !span.is_empty())
        .collect()
}

/// Removes single-backtick spans, so a link written as an example is not followed.
fn strip_code_spans(body: &str) -> String {
    body.lines()
        .map(|line| line.split('`').step_by(2).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every piece of `body` that sits between `open` and the next `close`.
fn between<'a>(body: &'a str, open: &str, close: char) -> Vec<&'a str> {
    body.split(open)
        .skip(1)
        .filter_map(|after| after.split_once(close))
        .map(|(inside, _)| inside)
        .collect()
}

/// Everything `body` links to or embeds: Markdown `](target)` (a `"title"` after the
/// target is dropped) and the HTML `src="…"` / `href="…"` that README uses.
fn link_targets(body: &str) -> Vec<&str> {
    let markdown = between(body, "](", ')')
        .into_iter()
        .filter_map(|inside| inside.split_whitespace().next());
    let html = between(body, "src=\"", '"')
        .into_iter()
        .chain(between(body, "href=\"", '"'));
    markdown.chain(html).filter(|t| !t.is_empty()).collect()
}

fn is_external(target: &str) -> bool {
    target.starts_with("http://") || target.starts_with("https://") || target.starts_with("mailto:")
}

const IMAGE_EXTS: &[&str] = &[".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"];

fn is_image(target: &str) -> bool {
    let lower = target.to_lowercase();
    IMAGE_EXTS.iter().any(|ext| lower.ends_with(ext))
}

/// Drops `<!-- … -->`, which the manual renderer removes before it reads anything else
/// (a heading inside a comment gets no id and takes no number in the duplicate count).
fn strip_html_comments(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Splits `page.md#anchor` into its page and its anchor.
fn split_anchor(target: &str) -> (&str, Option<&str>) {
    target
        .split_once('#')
        .map_or((target, None), |(page, anchor)| (page, Some(anchor)))
}

/// Resolves `target` relative to the directory of `from`. `None` if it leaves the repo.
fn resolve(from: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// Replaces `[label](url)` and `![alt](url)` by the label, as `stripInline` in
/// ui/manual.js does before a heading is slugged. Backticks and `**` need no handling
/// here: the slug filter drops them anyway.
fn strip_links(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|i| open + i) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|i| close + i) else {
            break;
        };
        out.push_str(rest[..open].trim_end_matches('!'));
        out.push_str(&rest[open + 1..close]);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Heading slugs, the way `makeSlugger` in ui/manual.js builds them (GitHub-compatible):
/// lowercase, keep letters / digits / `_` / `-` / spaces, spaces become `-`, and a
/// repeated slug gets `-1`, `-2`, ...
///
/// `char::is_alphanumeric` stands in for `\p{L}\p{N}\p{M}`. It does not cover combining
/// marks, which the manual's headings do not use.
///
/// The line handling follows `renderMarkdown` too, not GitHub, where they differ:
/// comments are removed first, a fence is ``` at column zero only, a heading is
/// `^#{1,6}\s+(.*)$`, and trailing spaces in it are kept (they become `-`).
fn heading_slugs(body: &str) -> HashSet<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut slugs = HashSet::new();
    let mut in_fence = false;
    for line in strip_html_comments(body).lines() {
        if line.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        let hashes = line.chars().take_while(|c| *c == '#').count();
        let text = line[hashes..].trim_start();
        if in_fence || !(1..=6).contains(&hashes) || text.len() == line[hashes..].len() {
            continue;
        }
        let mut slug: String = strip_links(text)
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ' '))
            .map(|c| if c == ' ' { '-' } else { c })
            .collect();
        let count = seen.entry(slug.clone()).or_insert(0);
        if *count > 0 {
            slug = format!("{slug}-{count}");
        }
        *count += 1;
        slugs.insert(slug);
    }
    slugs
}

// ── Symbols ──────────────────────────────────────────────────────────────────

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The symbol a code span names, if it is one: `name`, `name()`, `module::name`,
/// `Type.field`. Qualified names are matched by their last segment, because the prefix
/// is often spelled differently at the definition.
fn symbol_in(span: &str) -> Option<&str> {
    let span = span.strip_suffix("()").unwrap_or(span);
    let mut last = None;
    for qualified in span.split("::") {
        for segment in qualified.split('.') {
            if !is_ident(segment) {
                return None;
            }
            last = Some(segment);
        }
    }
    last
}

/// A lone lowercase word cannot be told from prose or a value literal, so only names
/// with an underscore, a camelCase step, or all capitals are checked.
fn looks_like_symbol(name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    name.contains('_')
        || chars
            .windows(2)
            .any(|w| w[0].is_ascii_lowercase() && w[1].is_ascii_uppercase())
        || chars
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Whether `file` may vouch for a symbol name. Left out:
/// - Markdown, so the notes cannot vouch for themselves;
/// - images and lock files, which hold no names of ours;
/// - the vendored libraries in `ui/lib/` (the same line biome.json draws). Their
///   minified code holds as many identifier-shaped tokens as all of muSQL, and a renamed
///   function of ours would pass whenever CodeMirror happens to spell one the same way;
/// - this module, whose allow lists and tests name things that do not exist.
fn vouches_for_names(file: &str) -> bool {
    let lower = file.to_lowercase();
    let skipped_ext = [".md", ".png", ".jpg", ".svg", ".ico", ".icns", ".lock"];
    !skipped_ext.iter().any(|ext| lower.ends_with(ext))
        && !file.starts_with("ui/lib/")
        && !file.ends_with("docs_consistency.rs")
}

/// Every identifier-shaped token in the files that may vouch for one.
fn corpus_names(files: &[String]) -> HashSet<String> {
    let mut names = HashSet::new();
    for file in files.iter().filter(|f| vouches_for_names(f)) {
        // Binary files, and files deleted in the worktree, are skipped.
        let Ok(body) = fs::read_to_string(root().join(file)) else {
            continue;
        };
        names.extend(
            body.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|token| !token.is_empty())
                .map(str::to_owned),
        );
    }
    names
}

fn report(what: &str, problems: BTreeSet<String>) {
    assert!(
        problems.is_empty(),
        "{what}: {} problem(s)\n  - {}\n",
        problems.len(),
        problems.into_iter().collect::<Vec<_>>().join("\n  - ")
    );
}

// ── 1. Paths in the dev notes ────────────────────────────────────────────────

#[test]
fn dev_notes_name_existing_paths() {
    let files = repo_files();
    let paths = repo_paths(&files);
    // A span is a path claim when it starts with one of the repository's top-level
    // directories. Derived, so a directory added later is covered without touching this.
    let top_dirs: BTreeSet<String> = paths
        .iter()
        .filter(|p| !p.contains('/') && paths.iter().any(|q| q.starts_with(&format!("{p}/"))))
        .map(|p| format!("{p}/"))
        .collect();
    assert!(
        top_dirs.contains("src-tauri/"),
        "top-level scan is broken: {top_dirs:?}"
    );

    let mut problems = BTreeSet::new();
    for file in note_files(&files) {
        let body = strip_fences(&read(&file));
        for span in code_spans(&body) {
            if !top_dirs.iter().any(|dir| span.starts_with(dir.as_str())) {
                continue;
            }
            // Globs, placeholders and command lines are not single paths.
            if span.contains(|c: char| c.is_whitespace() || "*{}<>$".contains(c)) {
                continue;
            }
            // `path#anchor`, `path/` and `path:line` all name `path`.
            let path = split_anchor(span).0.trim_end_matches('/');
            let path = match path.rsplit_once(':') {
                Some((file, line)) if line.chars().all(|c| c.is_ascii_digit()) => file,
                _ => path,
            };
            if !paths.contains(path) {
                problems.insert(format!(
                    "{file} names a path that is not in the repository: {path}"
                ));
            }
        }
    }
    report("paths in the dev notes", problems);
}

// ── 2. Symbols in the dev notes ──────────────────────────────────────────────

#[test]
fn dev_notes_name_existing_symbols() {
    let files = repo_files();
    let corpus = corpus_names(&files);
    let mut problems = BTreeSet::new();
    for file in note_files(&files) {
        let body = strip_fences(&read(&file));
        for span in code_spans(&body) {
            let Some(name) = symbol_in(span) else {
                continue;
            };
            if name.len() < 4
                || !looks_like_symbol(name)
                || corpus.contains(name)
                || EXTERNAL_NAMES.contains(&name)
                || GONE_NAMES.contains(&name)
            {
                continue;
            }
            problems.insert(format!(
                "{file} names a symbol that is not in the code: {name}"
            ));
        }
    }
    report(
        "symbols in the dev notes (renamed? fix the note. someone else's API? add it to \
         EXTERNAL_NAMES. named to say it is gone? GONE_NAMES)",
        problems,
    );
}

// ── 3. In-app help targets ───────────────────────────────────────────────────

/// How ui/manual.js spells the notation in its header comment. Not a real target.
const HELP_TARGET_NOTATION: &str = "page.md#anchor";

#[test]
fn help_buttons_point_at_existing_manual_headings() {
    let manual_js = read("ui/manual.js");
    assert!(
        manual_js.contains(r"[^\p{L}\p{N}\p{M}_\- ]"),
        "the slug rule in ui/manual.js changed; update heading_slugs() in this module to match"
    );

    let files = repo_files();
    let paths = repo_paths(&files);
    let mut problems = BTreeSet::new();
    let mut checked = 0;
    for source in files_in(&files, "ui", &[".html", ".js"]) {
        let body = read(&source);
        let mut targets = between(&body, "data-manual=\"", '"');
        targets.extend(between(&body, "createHelpButton(\"", '"'));
        targets.extend(between(&body, "createHelpButton('", '\''));
        for target in targets {
            // Nothing else is skipped: a target without `.md` is a typo, and the page
            // lookup below reports it.
            if target == HELP_TARGET_NOTATION {
                continue;
            }
            checked += 1;
            let (page, anchor) = split_anchor(target);
            let page_path = format!("docs/manual/{page}");
            if !paths.contains(&page_path) {
                problems.insert(format!("{source}: no manual page for {target}"));
            } else if anchor.is_some_and(|a| !heading_slugs(&read(&page_path)).contains(a)) {
                problems.insert(format!("{source}: no heading for {target}"));
            }
        }
    }
    assert!(
        checked > 0,
        "found no help targets at all; the scan is broken"
    );
    report("in-app help targets", problems);
}

// ── 4. Manual images ─────────────────────────────────────────────────────────

#[test]
fn manual_images_exist_and_are_all_referenced() {
    let files = repo_files();
    let paths = repo_paths(&files);
    let mut problems = BTreeSet::new();
    let mut referenced = HashSet::new();
    for file in doc_files(&files) {
        let body = strip_fences(&read(&file));
        for target in link_targets(&body) {
            if is_external(target) || !is_image(target) {
                continue;
            }
            match resolve(&file, target) {
                Some(path) if paths.contains(&path) => {
                    referenced.insert(path);
                }
                _ => {
                    problems.insert(format!("{file}: image not found -> {target}"));
                }
            }
        }
    }
    // Every image under the manual's image directory, at any depth and in any format.
    for image in files
        .iter()
        .filter(|f| f.starts_with("docs/manual/img/") && is_image(f))
    {
        if !referenced.contains(image) {
            problems.insert(format!("image referenced by no page: {image}"));
        }
    }
    report("manual images", problems);
}

// ── 5. Links and anchors ─────────────────────────────────────────────────────

#[test]
fn doc_links_and_anchors_resolve() {
    let files = repo_files();
    let paths = repo_paths(&files);
    let mut problems = BTreeSet::new();
    for file in doc_files(&files) {
        let body = strip_code_spans(&strip_fences(&read(&file)));
        for link in link_targets(&body) {
            if is_external(link) || is_image(link) {
                continue;
            }
            let (path, anchor) = split_anchor(link);
            let target = if path.is_empty() {
                file.clone()
            } else {
                match resolve(&file, path) {
                    Some(resolved) if paths.contains(&resolved) => resolved,
                    _ => {
                        problems.insert(format!("{file}: link target not found -> {link}"));
                        continue;
                    }
                }
            };
            if target.ends_with(".md")
                && anchor.is_some_and(|a| !heading_slugs(&read(&target)).contains(a))
            {
                problems.insert(format!("{file}: no such heading -> {link}"));
            }
        }
    }
    report("links between README and the manual", problems);
}

// ── The helpers themselves ───────────────────────────────────────────────────
//
// Checks 1 and 2 pass when they examine nothing, so the helpers that decide what gets
// examined are pinned here.

#[test]
fn heading_slugs_follow_the_manual_renderer() {
    let slugs = heading_slugs(
        "# `musql.*` ラベルでのカスタマイズ\n## 接続情報と SSL\n## 使い方\n## 使い方\n\
         ```\n## not a heading\n```\n## プロファイルの作成・編集\n## [リンク](x.md) 付き\n\
         <!--\n## 使い方\n-->\n#no-space\n",
    );
    // The commented-out heading must not take a number: there are two real ones only.
    assert!(!slugs.contains("使い方-2"));
    assert!(!slugs.contains("no-space"));
    for expected in [
        "musql-ラベルでのカスタマイズ",
        "接続情報と-ssl",
        "使い方",
        "使い方-1",
        "プロファイルの作成編集",
        "リンク-付き",
    ] {
        assert!(
            slugs.contains(expected),
            "missing slug {expected}: {slugs:?}"
        );
    }
    assert!(!slugs.contains("not-a-heading"));
}

#[test]
fn code_spans_and_links_are_extracted() {
    assert_eq!(
        code_spans("a `one` b `two`\n`three`"),
        ["one", "two", "three"]
    );
    assert_eq!(
        strip_code_spans("see `[x](y.md)` and [z](w.md)"),
        "see  and [z](w.md)"
    );
    assert_eq!(
        link_targets(r#"[a](b.md) ![i](img/c.png "title") <img src="d.svg"> <a href="LICENSE">"#),
        ["b.md", "img/c.png", "d.svg", "LICENSE"]
    );
    assert_eq!(
        strip_html_comments("a <!-- x\n## y\n--> b <!-- open"),
        "a  b "
    );
    assert_eq!(
        between(r#"<b data-manual="x.md#y">"#, "data-manual=\"", '"'),
        ["x.md#y"]
    );
    assert_eq!(
        resolve("docs/manual/a.md", "../../README.md").as_deref(),
        Some("README.md")
    );
    assert_eq!(resolve("README.md", "../x.md"), None);
}

#[test]
fn symbol_in_reads_qualified_names_and_rejects_the_rest() {
    assert_eq!(symbol_in("is_debug_build"), Some("is_debug_build"));
    assert_eq!(symbol_in("app_log::warn_throttled"), Some("warn_throttled"));
    assert_eq!(
        symbol_in("window.createHelpButton()"),
        Some("createHelpButton")
    );
    assert_eq!(symbol_in("just check"), None);
    assert_eq!(symbol_in("musql:notify-query"), None);
    assert_eq!(symbol_in("src-tauri/src/main.rs"), None);
}

#[test]
fn looks_like_symbol_skips_plain_words() {
    assert!(looks_like_symbol("save_profiles"));
    assert!(looks_like_symbol("openQuickOpen"));
    assert!(looks_like_symbol("RUNNING_QUERIES"));
    assert!(!looks_like_symbol("password"));
    assert!(!looks_like_symbol("Docker"));
}

#[test]
fn vendored_code_and_notes_do_not_vouch_for_names() {
    assert!(vouches_for_names("src-tauri/src/main.rs"));
    assert!(vouches_for_names("ui/query.js"));
    assert!(vouches_for_names(".github/workflows/release.yml"));
    assert!(!vouches_for_names("ui/lib/codemirror/codemirror.min.js"));
    assert!(!vouches_for_names("CLAUDE.md"));
    assert!(!vouches_for_names("src-tauri/Cargo.lock"));
    assert!(!vouches_for_names("src-tauri/src/docs_consistency.rs"));
}
