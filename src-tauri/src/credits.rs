//! The licence list shown in "About muSQL" (#119): `ui/credits.json`.
//!
//! The file is generated and **committed**. Building it during the build would tie the
//! build to the contents of cargo's registry; a committed file is also what gets
//! reviewed. Two tests live here:
//!
//! - `credits_match_the_dependencies` runs with `just test` and in CI, and fails when
//!   the file no longer describes what is shipped.
//! - `generate` is `#[ignore]`d and rewrites the file: `just credits` (which `just bump`
//!   runs too, so a release always ships a fresh list).
//!
//! A test module rather than a script because the project does not require Node.js, and
//! rather than a second binary because muSQL is a binary-only crate (same reasoning as
//! `verify.rs`).
//!
//! ## What is listed
//!
//! - Rust crates reachable from muSQL through *normal* dependencies, for the target
//!   that is released (`TARGET`), with the default features. Build- and
//!   dev-dependencies are not in the binary and are left out. Proc-macro crates are
//!   normal dependencies and stay in: more than is strictly shipped, never less. The Store build only
//!   *drops* a feature, so its dependencies are a subset of this list. **A new release
//!   target, or a feature that is not in `default`, has to be added here**, or its
//!   dependencies go unlisted without anything failing.
//! - What is vendored under `ui/lib/`, and the Lucide icons in `ui/icons.js`. Those are
//!   listed by hand in `VENDORED`; their licence files sit under `ui/lib/`.
//!
//! ## What the check compares
//!
//! For crates: **names and licences, not versions.** On purpose. A dependabot update
//! changes versions every week; if that failed this check, no dependabot PR could ever
//! be merged as it is. What a licence list has to get right is *which* library is under
//! *which* licence, so the check fails when a crate is added or removed or its licence
//! changes. The versions are refreshed whenever the file is regenerated.
//!
//! For vendored code the version is compared as well: dependabot never touches
//! `ui/lib/`, so there a mismatch can only mean that `VENDORED` was edited and the file
//! was not regenerated.
//!
//! ## Licence texts
//!
//! Read from each package's own `LICENSE` / `COPYING` / `NOTICE` files (and the
//! `license-file` its manifest names), with identical texts stored once. A package that
//! ships no such file gets the standard SPDX text for its licence instead, marked
//! `standard`, with its authors alongside (what cargo-about does too). Fetching those is
//! the only time `generate` needs the network.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const OUT: &str = "ui/credits.json";

/// The target that is released (`.github/workflows/release.yml`: both jobs build on
/// `windows-latest` with no `--target`). Dependencies that only other platforms pull in
/// are not in the binary.
const TARGET: &str = "x86_64-pc-windows-msvc";

/// Where the standard texts come from. Pinned to a release so that regenerating does
/// not change them.
const SPDX_TEXT: &str = "https://raw.githubusercontent.com/spdx/license-list-data/v3.27.0/text";

/// Shown as the licence of a package whose manifest declares none; its text is then the
/// thing to read.
const UNKNOWN_LICENSE: &str = "see license text";

/// A shipped text shorter than this is a notice that points at a licence, not the
/// licence. (The shortest licences listed, ISC and 0BSD, are longer than this; the
/// notice this was written for is 280 characters.)
const FULL_TEXT_MIN: usize = 500;

/// Where vendored code lives. Every directory in it must have an entry in `VENDORED`.
const VENDOR_DIR: &str = "ui/lib";

/// Vendored code: name, version, licence, URL, licence file (repo-relative).
///
/// **When something under `ui/lib/` is added or updated, or icons are copied into
/// `ui/icons.js`, update this list and the LICENSE file with it, then run
/// `just credits`.** The check below catches a missing directory entry, a missing
/// licence file, a CodeMirror version that differs from the vendored file, and a list
/// that was edited without regenerating. It cannot know the other versions.
const VENDORED: &[(&str, &str, &str, &str, &str)] = &[
    (
        "CodeMirror",
        "5.65.21",
        "MIT",
        "https://codemirror.net/5/",
        "ui/lib/codemirror/LICENSE",
    ),
    (
        "sql-formatter",
        "15.4.10",
        "MIT",
        "https://github.com/sql-formatter-org/sql-formatter",
        "ui/lib/sql-formatter/LICENSE",
    ),
    (
        // The icon paths in ui/icons.js; there is no code under ui/lib/lucide, only the
        // licence. No version was recorded when the icons were copied.
        "Lucide",
        "",
        "ISC",
        "https://lucide.dev",
        "ui/lib/lucide/LICENSE",
    ),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a parent")
        .to_path_buf()
}

/// One shipped crate, as cargo describes it.
struct Crate {
    name: String,
    version: String,
    /// The SPDX expression from the manifest, when it declares one.
    license: Option<String>,
    /// A licence file the manifest points at (`license-file`), when it does.
    license_file: Option<PathBuf>,
    url: String,
    authors: Vec<String>,
    dir: PathBuf,
}

impl Crate {
    /// What is shown, and compared, as this crate's licence.
    fn license_label(&self) -> String {
        self.license
            .clone()
            .unwrap_or_else(|| UNKNOWN_LICENSE.to_owned())
    }
}

/// The crates that end up in the released binary.
fn shipped_crates() -> Vec<Crate> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--filter-platform",
            TARGET,
        ])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")))
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let meta: Value = serde_json::from_slice(&output.stdout).expect("cargo metadata is JSON");

    let str_of = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    let non_empty = |v: &Value| Some(str_of(v)).filter(|s| !s.is_empty());
    let nodes: BTreeMap<String, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .expect("resolve.nodes")
        .iter()
        .map(|n| (str_of(&n["id"]), n))
        .collect();
    let packages: BTreeMap<String, &Value> = meta["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|p| (str_of(&p["id"]), p))
        .collect();

    // Walk from muSQL along normal dependencies only: a `kind` of null. "build" and
    // "dev" edges lead to code that is not shipped.
    let root_id = str_of(&meta["resolve"]["root"]);
    let mut seen = BTreeSet::new();
    let mut stack = vec![root_id.clone()];
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for dep in nodes[&id]["deps"].as_array().into_iter().flatten() {
            let normal = dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k["kind"].is_null());
            if normal {
                stack.push(str_of(&dep["pkg"]));
            }
        }
    }
    seen.remove(&root_id);

    seen.iter()
        .map(|id| {
            let p = packages[id];
            let name = str_of(&p["name"]);
            let dir = Path::new(&str_of(&p["manifest_path"]))
                .parent()
                .expect("a manifest has a directory")
                .to_path_buf();
            Crate {
                version: str_of(&p["version"]),
                license: non_empty(&p["license"]),
                license_file: non_empty(&p["license_file"]).map(|f| dir.join(f)),
                // The dialog opens the URL in the browser, and only https is allowed
                // there; anything else falls through to the crates.io page.
                url: [&p["repository"], &p["homepage"]]
                    .into_iter()
                    .filter_map(non_empty)
                    .find(|u| u.starts_with("https://"))
                    .unwrap_or_else(|| format!("https://crates.io/crates/{name}")),
                authors: p["authors"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(str_of)
                    .collect(),
                dir,
                name,
            }
        })
        .collect()
}

/// What the check compares, one string per listed thing: `name | licence` for a crate,
/// `name version | licence` for vendored code (see the module doc for why they differ).
fn expected_entries() -> BTreeSet<String> {
    shipped_crates()
        .iter()
        .map(|c| format!("{} | {}", c.name, c.license_label()))
        .chain(
            VENDORED
                .iter()
                .map(|(name, version, license, _, _)| format!("{name} {version} | {license}")),
        )
        .collect()
}

/// The same key, read back from an entry of the committed file.
fn listed_entry(p: &Value, vendored: bool) -> String {
    let field = |key: &str| p[key].as_str().unwrap_or_default().to_owned();
    if vendored {
        format!(
            "{} {} | {}",
            field("name"),
            field("version"),
            field("license")
        )
    } else {
        format!("{} | {}", field("name"), field("license"))
    }
}

/// Whether a file name is a licence file: `LICENSE`, `LICENSE-MIT`, `COPYING.txt`,
/// `NOTICE`, `UNLICENSE`, `COPYRIGHT`, in any case, with the British spelling too.
fn is_license_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "license",
        "licence",
        "copying",
        "notice",
        "unlicense",
        "copyright",
    ]
    .iter()
    .any(|stem| {
        lower
            .strip_prefix(stem)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['-', '.', '_']))
    })
}

/// A text as it is stored: `\n` line ends, no surrounding blank space. Normalised so
/// that the same licence from two packages compares equal.
fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_owned()
}

/// The licence texts a crate ships: the file its manifest names, if any, then the files
/// in its directory that are named like a licence, in file-name order.
fn shipped_texts(c: &Crate) -> Vec<String> {
    let mut files: Vec<PathBuf> = c.license_file.iter().cloned().collect();
    let mut by_name: Vec<PathBuf> = fs::read_dir(&c.dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_license_file)
        })
        .collect();
    by_name.sort();
    for path in by_name {
        if !files.contains(&path) {
            files.push(path);
        }
    }
    files
        .iter()
        .filter_map(|path| fs::read(path).ok())
        .map(|bytes| normalise(&String::from_utf8_lossy(&bytes)))
        .filter(|t| !t.is_empty())
        .collect()
}

/// The SPDX identifiers in a licence expression: `MIT OR Apache-2.0`, `MIT/Apache-2.0`,
/// `(MIT OR Apache-2.0) AND Unicode-3.0`. For an `OR`, every alternative is returned:
/// the list does not claim which one was chosen, so it carries them all.
fn spdx_ids(expression: &str) -> Vec<String> {
    expression
        .replace(['(', ')', '/'], " ")
        .split_whitespace()
        .filter(|word| !matches!(*word, "OR" | "AND" | "WITH"))
        .map(str::to_owned)
        .collect()
}

/// Stores each distinct text once and returns its index.
fn intern(texts: &mut Vec<String>, text: String) -> usize {
    match texts.iter().position(|t| *t == text) {
        Some(i) => i,
        None => {
            texts.push(text);
            texts.len() - 1
        }
    }
}

/// Fetches the standard text of one SPDX licence.
fn fetch_standard_text(client: &reqwest::Client, id: &str) -> String {
    let url = format!("{SPDX_TEXT}/{id}.txt");
    let body = tauri::async_runtime::block_on(async {
        let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        resp.text().await.map_err(|e| e.to_string())
    })
    .unwrap_or_else(|e| panic!("could not fetch the standard text for {id} ({url}): {e}"));
    normalise(&body)
}

/// Rewrites `ui/credits.json`. Run it with `just credits` after the dependencies change.
#[test]
#[ignore]
fn generate() {
    let client = reqwest::Client::new();
    let mut texts: Vec<String> = Vec::new();
    // SPDX id -> index of its standard text, so each one is fetched once.
    let mut standard: BTreeMap<String, usize> = BTreeMap::new();
    let mut packages = Vec::new();

    for c in shipped_crates() {
        let own: Vec<usize> = shipped_texts(&c)
            .into_iter()
            .map(|t| intern(&mut texts, t))
            .collect();
        let mut entry = json!({
            "name": c.name,
            "version": c.version,
            "license": c.license_label(),
            "url": c.url,
        });
        // A package may ship only a short notice ("licensed under MIT or Apache-2.0, see
        // ...") and no licence itself. That does not count as shipping the text.
        let has_full_text = own.iter().any(|&i| texts[i].len() >= FULL_TEXT_MIN);
        if !has_full_text {
            // No licence text in the package: add the standard texts of its licence(s),
            // and say whose copyright it is.
            let ids = c.license.as_deref().map(spdx_ids).unwrap_or_default();
            assert!(
                !(ids.is_empty() && own.is_empty()),
                "{} {} ships no licence file and declares no licence. Its licence has to be \
                 found by hand; it cannot be listed automatically.",
                c.name,
                c.version
            );
            let mut indices = own;
            for id in &ids {
                let index = *standard.entry(id.clone()).or_insert_with(|| {
                    println!("credits: fetching the standard text of {id}");
                    intern(&mut texts, fetch_standard_text(&client, id))
                });
                if !indices.contains(&index) {
                    indices.push(index);
                }
            }
            entry["texts"] = json!(indices);
            entry["standard"] = json!(true);
            entry["authors"] = json!(c.authors);
        } else {
            entry["texts"] = json!(own);
        }
        packages.push(entry);
    }

    for (name, version, license, url, file) in VENDORED {
        let text = fs::read_to_string(root().join(file))
            .unwrap_or_else(|e| panic!("cannot read {file}: {e}"));
        packages.push(json!({
            "name": name,
            "version": version,
            "license": license,
            "url": url,
            "vendored": true,
            "texts": [intern(&mut texts, normalise(&text))],
        }));
    }

    // Case-insensitive, so that "CodeMirror" sorts among the crates and not before them.
    packages.sort_by_key(|p| {
        (
            p["name"].as_str().unwrap_or_default().to_lowercase(),
            p["version"].as_str().unwrap_or_default().to_owned(),
        )
    });

    let out = json!({ "packages": packages, "texts": texts });
    let mut body = serde_json::to_string_pretty(&out).expect("serialises");
    body.push('\n');
    fs::write(root().join(OUT), body).unwrap_or_else(|e| panic!("cannot write {OUT}: {e}"));
    println!(
        "credits: wrote {OUT} ({} packages, {} texts)",
        packages.len(),
        texts.len()
    );
}

/// Fails when `ui/credits.json` no longer describes what is shipped.
#[test]
fn credits_match_the_dependencies() {
    let body = fs::read_to_string(root().join(OUT))
        .unwrap_or_else(|e| panic!("cannot read {OUT}: {e}. Run `just credits`."));
    let credits: Value = serde_json::from_str(&body).expect("credits.json is JSON");
    let packages = credits["packages"].as_array().expect("packages is a list");
    let text_count = credits["texts"].as_array().expect("texts is a list").len();

    let mut listed = BTreeSet::new();
    for p in packages {
        let name = p["name"].as_str().expect("name");
        let texts = p["texts"].as_array().expect("texts");
        assert!(!texts.is_empty(), "{name} has no licence text");
        for i in texts {
            let i = i.as_u64().expect("a text index") as usize;
            assert!(
                i < text_count,
                "{name} refers to text {i}, which does not exist"
            );
        }
        listed.insert(listed_entry(p, p["vendored"].as_bool().unwrap_or(false)));
    }

    let expected = expected_entries();
    let missing: Vec<_> = expected.difference(&listed).collect();
    let stale: Vec<_> = listed.difference(&expected).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "{OUT} is out of date. Run `just credits` and commit the result.\n  \
         not listed yet: {missing:?}\n  no longer shipped: {stale:?}"
    );
}

/// `VENDORED` is written by hand; this is what can be checked about it mechanically.
#[test]
fn vendored_list_covers_what_is_vendored() {
    // Every licence file exists.
    for (name, _, _, _, file) in VENDORED {
        assert!(
            root().join(file).is_file(),
            "{name}: licence file {file} is missing"
        );
    }

    // Every directory under ui/lib has an entry (its licence file sits in it).
    let covered: BTreeSet<PathBuf> = VENDORED
        .iter()
        .filter_map(|(_, _, _, _, file)| Path::new(file).parent().map(Path::to_path_buf))
        .collect();
    for entry in fs::read_dir(root().join(VENDOR_DIR)).expect("ui/lib is listable") {
        let path = entry.expect("dir entry").path();
        if !path.is_dir() {
            continue;
        }
        let rel = Path::new(VENDOR_DIR).join(path.file_name().expect("a name"));
        assert!(
            covered.contains(&rel),
            "{} is vendored but has no entry in VENDORED (src-tauri/src/credits.rs)",
            rel.display()
        );
    }

    // CodeMirror's vendored files say which version they are.
    let version = VENDORED
        .iter()
        .find(|(name, ..)| *name == "CodeMirror")
        .map(|(_, version, ..)| *version)
        .expect("CodeMirror is vendored");
    let header = fs::read_to_string(root().join("ui/lib/codemirror/codemirror.min.js"))
        .expect("codemirror.min.js is readable");
    let header: String = header.chars().take(400).collect();
    assert!(
        header.contains(&format!("codemirror@{version}/")),
        "VENDORED says CodeMirror {version}, but ui/lib/codemirror/codemirror.min.js is another \
         version. Update VENDORED and the LICENSE file, then run `just credits`."
    );
}

#[test]
fn license_files_are_recognised() {
    for name in [
        "LICENSE",
        "LICENSE-MIT",
        "license.md",
        "LICENCE.txt",
        "COPYING",
        "NOTICE",
        "UNLICENSE",
        "COPYRIGHT",
        "LICENSE_APACHE",
    ] {
        assert!(is_license_file(name), "{name} should count");
    }
    for name in ["Cargo.toml", "licenses", "README.md", "noticeboard.rs"] {
        assert!(!is_license_file(name), "{name} should not count");
    }
}

#[test]
fn spdx_ids_are_split_out_of_an_expression() {
    assert_eq!(spdx_ids("MIT OR Apache-2.0"), ["MIT", "Apache-2.0"]);
    assert_eq!(spdx_ids("MIT/Apache-2.0"), ["MIT", "Apache-2.0"]);
    assert_eq!(
        spdx_ids("(MIT OR Apache-2.0) AND Unicode-3.0"),
        ["MIT", "Apache-2.0", "Unicode-3.0"]
    );
    assert_eq!(spdx_ids("MPL-2.0"), ["MPL-2.0"]);
}

#[test]
fn identical_texts_are_stored_once() {
    let mut texts = Vec::new();
    let a = intern(&mut texts, normalise("MIT License\r\n\r\ntext\r\n"));
    let b = intern(&mut texts, normalise("  MIT License\n\ntext\n\n"));
    let c = intern(&mut texts, normalise("another"));
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(texts.len(), 2);
}
