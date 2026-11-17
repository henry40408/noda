//! Key sequences on a fixed notebook, and the frame each one leaves.
//!
//! The notes are written with fixed ids and stamps so a frame is the same on
//! every run. Screens that print a commit time (log, blame, diff) are left out
//! for that reason: libgit2 stamps a commit with the clock.

use std::path::Path;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

use super::{capture_topic, text};
use crate::notebook::Notebook;
use crate::paths::Paths;
use crate::tui::app::App;
use crate::{cmd, tui};

const NOTES: [(&str, &str); 4] = [
    (
        "01abc234-budget-review.md",
        "---\ntitle: Budget review\ntags: [work]\ncreated: 2024-01-10T09:00:00Z\nupdated: 2024-03-01T10:00:00Z\n---\n\nthe q3 budget is late\n\n- [ ] pay the bill due:2024-01-02\n- [ ] plan ahead due:2099-01-01\n- [x] done already\n",
    ),
    (
        "02def567-meeting-notes.md",
        "---\ntitle: Meeting notes\ntags: [work, q3]\ncreated: 2024-02-01T10:00:00+08:00\nupdated: 2024-02-02T10:00:00+08:00\npinned: true\n---\n\n# Agenda\n\nthe budget, again\n\nsee [the budget](01abc234-budget-review.md)\n",
    ),
    (
        "03ghj890-reading-list.md",
        "---\ntitle: Reading list\ncreated: 2024-01-05T00:00:00Z\nupdated: 2024-01-06T00:00:00Z\n---\n\na book\n",
    ),
    (
        "04kmn123-日本語メモ.md",
        "---\ntitle: 日本語メモ\ntags: [日本語, has space]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\n---\n\n本文はここにあります。\n\n- [ ] 日本語の用事 due:2024-05-06\n",
    ),
];

fn fixture(root: &Path) -> Paths {
    let paths = Paths::rooted(root);
    std::fs::create_dir_all(paths.config_dir()).expect("config dir");
    std::fs::write(paths.config_dir().join("config.toml"), "sign = false\n").expect("config");
    cmd::init(&paths).expect("init");
    let notebook = Notebook::open_active(&paths).expect("open");
    for (file, body) in NOTES {
        std::fs::write(notebook.path.join(file), body).expect("note");
    }
    std::fs::write(notebook.path.join("diagram.png"), b"not really a png").expect("file");
    notebook.commit_all("add notes").expect("commit");
    paths
}

fn key_event(token: &str) -> KeyEvent {
    let plain = |code| KeyEvent::new(code, KeyModifiers::NONE);
    if let Some(rest) = token.strip_prefix("C-") {
        let c = rest.chars().next().expect("key");
        return KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    }
    if let Some(rest) = token.strip_prefix("A-") {
        let c = rest.chars().next().expect("key");
        return KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT);
    }
    match token {
        "Enter" => plain(KeyCode::Enter),
        "Esc" => plain(KeyCode::Esc),
        "Tab" => plain(KeyCode::Tab),
        "BackTab" => KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        "Up" => plain(KeyCode::Up),
        "Down" => plain(KeyCode::Down),
        "Left" => plain(KeyCode::Left),
        "Right" => plain(KeyCode::Right),
        "PageUp" => plain(KeyCode::PageUp),
        "PageDown" => plain(KeyCode::PageDown),
        "Home" => plain(KeyCode::Home),
        "End" => plain(KeyCode::End),
        "Backspace" => plain(KeyCode::Backspace),
        "Delete" => plain(KeyCode::Delete),
        "Space" => plain(KeyCode::Char(' ')),
        one if one.chars().count() == 1 => plain(KeyCode::Char(one.chars().next().expect("char"))),
        other => panic!("unknown key {other}"),
    }
}

fn press(app: &mut App, token: &str) {
    if let Some(typed) = token.strip_prefix("type:") {
        for c in typed.chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        return;
    }
    app.on_key(key_event(token));
}

/// Runs of cells that share a non-default style: `[start, length, "fg=Yellow mod=BOLD"]`.
fn style_runs(buffer: &ratatui::buffer::Buffer, y: u16) -> Vec<Value> {
    let describe = |x: u16| -> String {
        let style = buffer[(x, y)].style();
        let mut parts = Vec::new();
        if let Some(fg) = style.fg {
            parts.push(format!("fg={fg:?}"));
        }
        if let Some(bg) = style.bg {
            parts.push(format!("bg={bg:?}"));
        }
        if !style.add_modifier.is_empty() {
            parts.push(format!("add={:?}", style.add_modifier));
        }
        if !style.sub_modifier.is_empty() {
            parts.push(format!("sub={:?}", style.sub_modifier));
        }
        parts.join(" ")
    };
    let mut runs: Vec<Value> = Vec::new();
    let width = buffer.area.width;
    let mut x = 0;
    while x < width {
        let here = describe(x);
        let mut end = x + 1;
        while end < width && describe(end) == here {
            end += 1;
        }
        if !here.is_empty() {
            runs.push(json!([x, end - x, here]));
        }
        x = end;
    }
    runs
}

fn frame(paths: &Paths, app: &mut App, width: u16, height: u16) -> Value {
    tui::refresh(paths, app);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal.draw(|f| tui::view::draw(f, app)).expect("draw");
    let cursor: (u16, u16) = terminal.get_cursor_position().expect("cursor").into();
    let buffer = terminal.backend().buffer().clone();
    let rows: Vec<Value> = (0..buffer.area.height)
        .map(|y| {
            let line: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            json!({"text": line.trim_end(), "styles": style_runs(&buffer, y)})
        })
        .collect();
    json!({"rows": rows, "cursor": cursor})
}

pub(super) fn capture() {
    let root = std::env::temp_dir().join(format!("noda-golden-tui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = fixture(&root);
    capture_topic("tui", |case| {
        let mut app = tui::load(&paths).expect("load");
        for token in case["keys"].as_array().expect("keys") {
            press(&mut app, token.as_str().expect("key"));
        }
        let width = u16::try_from(case["width"].as_u64().expect("width")).expect("u16");
        let height = u16::try_from(case["height"].as_u64().expect("height")).expect("u16");
        let _ = text(case, "name");
        frame(&paths, &mut app, width, height)
    });
    let _ = std::fs::remove_dir_all(&root);
}
