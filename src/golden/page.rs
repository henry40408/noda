//! The `web::page` functions, driven from `testdata/golden/page/cases.json`.

use serde_json::{Value, json};

use super::{capture_topic, text};
use crate::web::page::{
    self, About, Asked, Book, Draft, Errand, Held, Listing, Order, Reading, Row, Standing, Subject,
    Tally, Task,
};

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_str().expect("str").to_string())
        .collect()
}

fn opt(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn num(value: &Value) -> usize {
    usize::try_from(value.as_u64().expect("number")).expect("fits")
}

fn each<T>(value: &Value, make: impl Fn(&Value) -> T) -> Vec<T> {
    value.as_array().expect("array").iter().map(make).collect()
}

fn rows(value: &Value) -> Vec<Row> {
    each(value, |r| Row {
        id: text(r, "id").to_string(),
        title: text(r, "title").to_string(),
        tags: strings(&r["tags"]),
        stamp: opt(&r["stamp"]),
        shown: r["shown"].as_bool().expect("shown"),
        pinned: r["pinned"].as_bool().expect("pinned"),
    })
}

fn about(value: &Value) -> About {
    About::of(text(value, "id"), text(value, "slug"), text(value, "title"))
}

fn subject(value: &Value) -> Subject {
    Subject {
        what: text(value, "what").to_string(),
        at: text(value, "at").to_string(),
        mono: value["mono"].as_bool().expect("mono"),
    }
}

fn reading(value: &Value) -> Reading {
    Reading {
        id: text(value, "id").to_string(),
        slug: text(value, "slug").to_string(),
        title: text(value, "title").to_string(),
        tags: strings(&value["tags"]),
        created: opt(&value["created"]),
        updated: opt(&value["updated"]),
        rendered: text(value, "rendered").to_string(),
        pinned: value["pinned"].as_bool().expect("pinned"),
    }
}

fn standing(value: &Value) -> Standing {
    Standing {
        branch: text(value, "branch").to_string(),
        notes: num(&value["notes"]),
        files: num(&value["files"]),
        uncommitted: num(&value["uncommitted"]),
        remote: opt(&value["remote"]),
        drift: text(value, "drift").to_string(),
        problems: strings(&value["problems"]),
    }
}

fn books(value: &Value) -> Vec<Book> {
    each(value, |b| Book {
        name: text(b, "name").to_string(),
        notes: num(&b["notes"]),
        files: num(&b["files"]),
        uncommitted: num(&b["uncommitted"]),
        drift: opt(&b["drift"]),
        active: b["active"].as_bool().expect("active"),
        last: text(b, "last").to_string(),
    })
}

fn sort_named(name: &str) -> crate::cmd::Sort {
    match name {
        "slug" => crate::cmd::Sort::Slug,
        "created" => crate::cmd::Sort::Created,
        "updated" => crate::cmd::Sort::Updated,
        "title" => crate::cmd::Sort::Title,
        other => panic!("unknown sort {other}"),
    }
}

fn listing_page(case: &Value, draw: impl Fn(&Listing<'_>, Option<&str>) -> String) -> Value {
    let rows = rows(&case["rows"]);
    let asked = &case["asked"];
    let grouping: Vec<Vec<String>> = each(&asked["grouping"], strings);
    let terms = strings(&asked["terms"]);
    let problem = opt(&asked["problem"]);
    let asked = Asked {
        typed: text(asked, "typed"),
        grouping: &grouping,
        terms: &terms,
        problem: problem.as_deref(),
    };
    let order = Order {
        sort: sort_named(text(&case["order"], "sort")),
        reversed: case["order"]["reversed"].as_bool().expect("reversed"),
    };
    let list = Listing {
        book: text(case, "book"),
        rows: &rows,
        asked: &asked,
        order,
        drift: text(case, "drift"),
    };
    json!(draw(&list, case["front"].as_str()))
}

fn standing_page(case: &Value) -> Value {
    let standing_value = standing(&case["standing"]);
    let errand = case["errand"].as_object().map(|_| &case["errand"]);
    let errand = errand.map(|e| Errand {
        doing: text(e, "doing"),
        done: text(e, "done"),
        said: e["said"].as_str(),
        failed: e["failed"].as_bool().expect("failed"),
        seconds: e["seconds"].as_u64().expect("seconds"),
    });
    let book = text(case, "book");
    if text(case, "fn") == "standing" {
        json!(page::standing(book, &standing_value, errand.as_ref()))
    } else {
        json!(page::standing_main(book, &standing_value, errand.as_ref()))
    }
}

fn form_page(case: &Value) -> Value {
    let book = text(case, "book");
    let about = about(&case["about"]);
    match text(case, "fn") {
        "editing" => json!(page::editing(&page::Editing {
            book,
            about: &about,
            body: text(case, "body"),
            was: text(case, "was"),
            problem: case["problem"].as_str(),
        })),
        "clashed" => json!(page::clashed(&page::Clash {
            book,
            about: &about,
            theirs: text(case, "theirs"),
            mine: text(case, "mine"),
            now: text(case, "now"),
        })),
        "conflicted" => json!(page::conflicted(
            book,
            &about,
            text(case, "merged"),
            text(case, "now")
        )),
        "renaming" => json!(page::renaming(
            book,
            &about,
            text(case, "title"),
            case["problem"].as_str()
        )),
        "tagging" => json!(page::tagging(
            book,
            &about,
            &strings(&case["tags"]),
            case["problem"].as_str()
        )),
        "deleting" => json!(page::deleting(book, &about)),
        other => panic!("unknown form fn {other}"),
    }
}

pub(super) fn capture() {
    capture_topic("page", |case| {
        let book = case["book"].as_str().unwrap_or_default();
        match text(case, "fn") {
            "escape" => json!(page::escape(text(case, "text"))),
            "listing" => listing_page(case, |l, front| page::listing(l, front)),
            "listing_screen" => listing_page(case, |l, front| page::listing_screen(l, front)),
            "listing_pane" => listing_page(case, |l, _| page::listing_pane(l)),
            "notebooks" => json!(page::notebooks(&books(&case["books"]))),
            "files" => {
                let held = each(&case["held"], |h| Held {
                    name: text(h, "name").to_string(),
                    size: h["size"].as_u64().expect("size"),
                    kind: text(h, "kind").to_string(),
                    used: num(&h["used"]),
                });
                json!(page::files(book, &held))
            }
            "tags" => {
                let tallies = each(&case["tallies"], |t| Tally {
                    tag: text(t, "tag").to_string(),
                    notes: num(&t["notes"]),
                });
                json!(page::tags(book, &tallies))
            }
            "todo" => {
                let tasks = each(&case["tasks"], |t| Task {
                    id: text(t, "id").to_string(),
                    title: text(t, "title").to_string(),
                    text: text(t, "text").to_string(),
                    due: opt(&t["due"]),
                    overdue: t["overdue"].as_bool().expect("overdue"),
                });
                json!(page::todo(book, &tasks))
            }
            "backlinks" => json!(page::backlinks(
                book,
                &subject(&case["subject"]),
                &rows(&case["rows"])
            )),
            "backlinks_rows" => json!(page::backlinks_rows(
                book,
                &subject(&case["subject"]),
                &rows(&case["rows"])
            )),
            "note" => json!(page::note(
                book,
                &reading(&case["reading"]),
                text(case, "drift")
            )),
            "note_pane" => json!(page::note_pane(book, &reading(&case["reading"]))),
            "composing" => {
                let d = &case["draft"];
                let draft = Draft {
                    title: text(d, "title").to_string(),
                    tags: text(d, "tags").to_string(),
                    body: text(d, "body").to_string(),
                };
                json!(page::composing(book, &draft, case["problem"].as_str()))
            }
            "editing" | "clashed" | "conflicted" | "renaming" | "tagging" | "deleting" => {
                form_page(case)
            }
            "standing" | "standing_main" => standing_page(case),
            "failure" => json!(page::failure(text(case, "heading"), text(case, "detail"))),
            other => panic!("unknown page fn {other}"),
        }
    });
}
