//! Captures what this implementation prints for the Go rewrite to match.
//!
//! Test-only and ignored by default: `cargo test --lib golden::capture -- --ignored`
//! reads each `testdata/golden/<topic>/cases.json` and writes `expected.json`
//! beside it. The expected files are a record of Rust's behaviour, not a
//! specification; the Go tests compare against them case by case.

mod page;
mod tui;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::cmd::{display_width, format_time};
use crate::config::Config;
use crate::note::{self, Note};
use crate::paths::Paths;
use crate::query::{self, Query};
use crate::web::render::{self, Around};
use crate::{link, todo};

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/golden")
}

fn read_json(topic: &str, file: &str) -> Value {
    let path = golden_dir().join(topic).join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn write_pretty(topic: &str, file: &str, value: &Value) {
    let path = golden_dir().join(topic).join(file);
    let mut text = serde_json::to_string_pretty(value).expect("serialise");
    text.push('\n');
    std::fs::write(&path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

/// Runs `each` over every case and records `{name, out}` in the case order.
fn capture_topic(topic: &str, each: impl Fn(&Value) -> Value) {
    let cases = read_json(topic, "cases.json");
    let cases = cases.as_array().expect("cases.json is an array");
    assert!(cases.len() >= 30, "{topic}: fewer than 30 cases");
    let out: Vec<Value> = cases
        .iter()
        .map(|case| {
            // A panic is a Rust bug, not behaviour to copy: it is recorded so the
            // Go side can tell it from an answer, and skip the case.
            let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| each(case)))
                .unwrap_or_else(|_| json!({"rust_panic": true}));
            json!({"name": case["name"], "out": out})
        })
        .collect();
    write_pretty(topic, "expected.json", &Value::Array(out));
}

fn text<'a>(case: &'a Value, key: &str) -> &'a str {
    case[key]
        .as_str()
        .unwrap_or_else(|| panic!("case lacks `{key}`"))
}

fn outcome<T>(result: crate::Result<T>, ok: impl Fn(T) -> Value) -> Value {
    match result {
        Ok(value) => json!({"ok": ok(value)}),
        Err(e) => json!({"err": e.to_string()}),
    }
}

#[test]
#[ignore = "writes testdata/golden; run on purpose"]
fn capture() {
    clock();
    width();
    note_topic();
    link_topic();
    query_topic();
    todo_topic();
    config_topic();
    render_topic();
    wikitext_topic();
    merge3_topic();
    asset_topic();
    page::capture();
    tui::capture();
}

fn clock() {
    capture_topic("clock", |case| {
        let seconds = case["seconds"].as_i64().expect("seconds");
        let offset = i32::try_from(case["offset"].as_i64().expect("offset")).expect("offset");
        json!(format_time(seconds, offset))
    });
}

fn width() {
    capture_topic("width", |case| json!(display_width(text(case, "text"))));
}

fn note_topic() {
    capture_topic("note", |case| match text(case, "op") {
        "slugify" => json!(note::slugify(text(case, "title"))),
        "normalize_id" => json!(note::normalize_id(text(case, "input"))),
        "file_name" => json!(note::file_name(text(case, "id"), text(case, "slug"))),
        "split_stem" => json!(note::split_stem(text(case, "stem"))),
        "names_a_note" => json!(note::names_a_note(text(case, "name"))),
        "is_id_shaped" => json!(note::is_id_shaped(text(case, "text"))),
        "validate_title" => outcome(note::validate_title(text(case, "title")), |()| json!(true)),
        "validate_tag" => outcome(note::validate_tag(text(case, "tag")), |()| json!(true)),
        "roundtrip" => outcome(Note::parse(text(case, "text")), |n| {
            json!({
                "title": n.title, "tags": n.tags, "created": n.created, "updated": n.updated,
                "pinned": n.pinned, "extra": n.extra, "body": n.body,
                "is_pinned": n.is_pinned(), "rendered": n.render(),
            })
        }),
        "set_field" => json!(note::set_field(
            text(case, "text"),
            text(case, "key"),
            text(case, "value")
        )),
        "set_body" => json!(note::set_body(text(case, "text"), text(case, "body"))),
        other => panic!("unknown note op {other}"),
    });
}

fn link_topic() {
    capture_topic("link", |case| match text(case, "op") {
        "targets" => json!(
            link::targets(text(case, "body"))
                .into_iter()
                .collect::<Vec<_>>()
        ),
        "rewrite" => json!(link::rewrite(
            text(case, "body"),
            text(case, "old"),
            text(case, "new")
        )),
        "target" => json!(link::target(text(case, "dest"))),
        "scheme" => json!(link::scheme(text(case, "dest"))),
        other => panic!("unknown link op {other}"),
    });
}

fn query_fixture() -> Vec<(String, Note)> {
    let notes = read_json("query", "notes.json");
    notes
        .as_array()
        .expect("notes.json is an array")
        .iter()
        .map(|n| {
            let tags = n["tags"].as_array().expect("tags");
            let note = Note {
                title: text(n, "title").to_string(),
                tags: tags
                    .iter()
                    .map(|t| t.as_str().expect("tag").to_string())
                    .collect(),
                created: None,
                updated: None,
                pinned: n["pinned"].as_str().map(str::to_string),
                extra: Vec::new(),
                body: text(n, "body").to_string(),
            };
            (text(n, "id").to_string(), note)
        })
        .collect()
}

fn query_topic() {
    let notes = query_fixture();
    capture_topic("query", |case| {
        let tokens = query::split(text(case, "line"));
        let parsed = Query::parse(&tokens);
        let matched = |q: &Query| -> Vec<String> {
            notes
                .iter()
                .filter(|(id, n)| q.matches(id, n))
                .map(|(id, _)| id.clone())
                .collect()
        };
        json!({
            "tokens": tokens,
            "parse": outcome(parsed.map(|q| (matched(&q), q.grouping().to_vec(), q.excerpt_terms())),
                |(ids, grouping, terms)| json!({"matches": ids, "grouping": grouping, "excerpt_terms": terms})),
        })
    });
}

fn todo_topic() {
    capture_topic("todo", |case| {
        let items: Vec<Value> = todo::items(text(case, "body"))
            .into_iter()
            .map(|i| json!({"text": i.text, "due": i.due}))
            .collect();
        json!(items)
    });
}

/// A fresh root per case, so no case sees another's file.
fn config_topic() {
    capture_topic("config", |case| {
        let root = std::env::temp_dir().join(format!(
            "noda-golden-config-{}-{}",
            std::process::id(),
            text(case, "name")
        ));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::rooted(&root);
        std::fs::create_dir_all(paths.config_dir()).expect("config dir");
        let file = paths.config_dir().join("config.toml");
        match case["initial"].as_str() {
            Some("@template") => {
                Config::write_template(&paths).expect("template");
            }
            Some(initial) => std::fs::write(&file, initial).expect("write"),
            None => {}
        }
        let root_text = root.display().to_string();
        let mut results = Vec::new();
        for op in case["ops"].as_array().expect("ops") {
            let op: Vec<&str> = op
                .as_array()
                .expect("op")
                .iter()
                .map(|v| v.as_str().expect("str"))
                .collect();
            let result = Config::load(&paths).and_then(|mut config| match op[0] {
                "set" => config.set(op[1], op[2]).map(|()| json!("set")),
                "unset" => config.unset(op[1]).map(|removed| json!(removed)),
                other => panic!("unknown config op {other}"),
            });
            results.push(match result {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"err": e.to_string().replace(&root_text, "<ROOT>")}),
            });
        }
        let final_text = std::fs::read_to_string(&file).ok();
        let _ = std::fs::remove_dir_all(&root);
        json!({"results": results, "file": final_text})
    });
}

fn render_topic() {
    let around = read_json("render", "around.json");
    let named: Vec<(String, String)> = around["named"]
        .as_array()
        .expect("named")
        .iter()
        .map(|p| {
            (
                p[0].as_str().expect("id").to_string(),
                p[1].as_str().expect("slug").to_string(),
            )
        })
        .collect();
    let book = text(&around, "book").to_string();
    let around = Around::of(&book, &named);
    capture_topic("render", |case| {
        json!(render::body(text(case, "markdown"), &around))
    });
}

fn wikitext_topic() {
    let known: BTreeMap<String, String> =
        serde_json::from_value(read_json("wikitext", "known.json")).expect("known");
    capture_topic("wikitext", |case| {
        let resolve = |title: &str| known.get(title).cloned();
        let converted = crate::import::wikitext::convert(text(case, "text"), &resolve);
        json!({"text": converted.text, "left": converted.left.into_iter().collect::<Vec<_>>()})
    });
}

fn merge3_topic() {
    capture_topic("merge3", |case| {
        fn input(s: &str) -> git2::MergeFileInput<'_> {
            let mut input = git2::MergeFileInput::new();
            input.content(s.as_bytes());
            input
        }
        // Same call and labels as `web::merge`, which is private. libgit2 must be
        // initialised first, and hashing an object does that.
        git2::Oid::hash_object(git2::ObjectType::Blob, &[]).expect("init");
        let (base, ours, theirs) = (
            input(text(case, "base")),
            input(text(case, "ours")),
            input(text(case, "theirs")),
        );
        let mut options = git2::MergeFileOptions::new();
        options
            .our_label("what you wrote")
            .their_label("saved since");
        let merged = git2::merge_file(&base, &ours, &theirs, Some(&mut options)).expect("merge");
        json!({
            "text": String::from_utf8_lossy(merged.content()),
            "clean": merged.is_automergeable(),
        })
    });
}

/// Not a cases file: the set of assets is fixed, so the topic is the list of
/// addresses and one file per body, which the Go side embeds and hashes.
fn asset_topic() {
    use crate::web::asset::{self, Asset};
    let all = [
        Asset::Icon,
        Asset::Style,
        Asset::Listing,
        Asset::Standing,
        Asset::Panes,
        Asset::Beside,
        Asset::Stamps,
        Asset::Watching,
    ];
    let dir = golden_dir().join("asset");
    std::fs::create_dir_all(&dir).expect("asset dir");
    let mut out = Vec::new();
    for asset in all {
        let href = asset.href();
        let file = href.strip_prefix("/a/").expect("asset path");
        let held = asset::find(file).expect("held");
        std::fs::write(dir.join(file), &held.body).expect("write asset");
        out.push(json!({"href": href, "tag": asset.tag(), "kind": held.kind, "file": file, "bytes": held.body.len()}));
    }
    write_pretty("asset", "expected.json", &Value::Array(out));
}
