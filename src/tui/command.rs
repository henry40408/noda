//! What `:` accepts: noda's own subcommand names, since only a handful of them
//! are worth a key.
//!
//! A table rather than a `match` so `Ctrl-a` can list it; what each command does
//! is in `app`.

pub struct Spec {
    pub name: &'static str,
    /// Shortest first.
    pub aliases: &'static [&'static str],
    /// In the CLI help's notation: `<required>`, `[optional]`, `...`.
    pub takes: &'static str,
    pub what: &'static str,
}

impl Spec {
    pub fn usage(&self) -> String {
        if self.takes.is_empty() {
            self.name.to_string()
        } else {
            format!("{} {}", self.name, self.takes)
        }
    }
}

/// Getting about, then changing a note, then the notebook, then leaving.
pub const COMMANDS: &[Spec] = &[
    Spec {
        name: "open",
        aliases: &["o", "show"],
        takes: "<note>",
        what: "open a note by id or by slug",
    },
    Spec {
        name: "notes",
        aliases: &["ls"],
        takes: "[query...]",
        what: "back to the listing, filtered if a query is given",
    },
    // Screens showing what the subcommand of the same name prints.
    Spec {
        name: "todo",
        aliases: &["t"],
        takes: "",
        what: "every unticked box, soonest due first",
    },
    Spec {
        name: "tags",
        aliases: &[],
        takes: "",
        what: "every tag; enter filters the listing by one",
    },
    Spec {
        name: "backlinks",
        aliases: &["b"],
        takes: "[note]",
        what: "what links to a note — the one shown, or one named",
    },
    // The only command whose empty form means the notebook, not the note shown.
    Spec {
        name: "log",
        aliases: &["l"],
        takes: "[note]",
        what: "commits, newest first: the notebook's or a note's",
    },
    Spec {
        name: "blame",
        aliases: &[],
        takes: "[note]",
        what: "which commit put each line of a note where it is",
    },
    Spec {
        name: "diff",
        aliases: &[],
        takes: "",
        what: "what is uncommitted, or what the last commit did",
    },
    Spec {
        name: "deleted",
        aliases: &[],
        takes: "",
        what: "notes history holds that the notebook no longer does",
    },
    Spec {
        name: "files",
        aliases: &[],
        takes: "",
        what: "what the notebook holds that is not a note",
    },
    Spec {
        name: "notebooks",
        aliases: &["nb"],
        takes: "",
        what: "every notebook; enter moves this session to one",
    },
    Spec {
        name: "edit",
        aliases: &["e"],
        takes: "[note]",
        what: "open in $EDITOR: the one named, or the one shown",
    },
    Spec {
        name: "add",
        aliases: &["new"],
        takes: "[title...]",
        what: "make a note; no title takes it from the body",
    },
    Spec {
        name: "mv",
        aliases: &["retitle"],
        takes: "<title...>",
        what: "retitle the note on screen; the slug follows",
    },
    Spec {
        name: "tag",
        aliases: &[],
        takes: "[note] +tag -tag...",
        what: "add and remove tags: +work -q3 -\"two words\"",
    },
    Spec {
        name: "pin",
        aliases: &[],
        takes: "[note]",
        what: "float a note to the top of every listing",
    },
    Spec {
        name: "unpin",
        aliases: &[],
        takes: "[note]",
        what: "let a pinned note back down among the rest",
    },
    // Takes no note: the confirmation is only meaningful for a note on screen.
    Spec {
        name: "rm",
        aliases: &[],
        takes: "",
        what: "delete the note on screen, after a y",
    },
    // No confirmation: `restore` writes a new commit, so nothing is lost.
    Spec {
        name: "restore",
        aliases: &[],
        takes: "<note> <rev>",
        what: "put a note back as it was at a revision",
    },
    Spec {
        name: "use",
        aliases: &[],
        takes: "<notebook>",
        what: "move this session to another notebook",
    },
    Spec {
        name: "status",
        aliases: &[],
        takes: "",
        what: "notes, changes, and drift from the remote",
    },
    Spec {
        name: "doctor",
        aliases: &[],
        takes: "[--links] [--times]",
        what: "diagnose. Reports only; it changes nothing",
    },
    Spec {
        name: "snapshot",
        aliases: &[],
        takes: "[name]",
        what: "mark the notebook, or list what is marked",
    },
    Spec {
        name: "readme",
        aliases: &[],
        takes: "",
        what: "write the notebook's front page",
    },
    Spec {
        name: "sync",
        aliases: &[],
        takes: "",
        what: "pull, then push. Commits pending changes",
    },
    Spec {
        name: "push",
        aliases: &[],
        takes: "",
        what: "send this notebook's commits to the remote",
    },
    Spec {
        name: "pull",
        aliases: &[],
        takes: "",
        what: "bring in the remote's commits",
    },
    Spec {
        name: "reload",
        aliases: &["r"],
        takes: "",
        what: "read the notebook again",
    },
    Spec {
        name: "keys",
        aliases: &["help"],
        takes: "",
        what: "the keys this screen answers to",
    },
    Spec {
        name: "quit",
        aliases: &["q"],
        takes: "",
        what: "leave, asking first about an unsent queue",
    },
];

/// Exact only: a prefix like `pu` is ambiguous, and guessing wrong is a
/// network call nobody asked for.
pub fn find(name: &str) -> Option<&'static Spec> {
    COMMANDS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
}

/// Searches descriptions too, so `remote` finds `push` and `pull`.
pub fn matching(filter: &str) -> impl Iterator<Item = &'static Spec> {
    let filter = filter.trim().to_lowercase();
    COMMANDS.iter().filter(move |spec| {
        filter.is_empty()
            || spec.name.contains(&filter)
            || spec.aliases.iter().any(|alias| alias.contains(&filter))
            || spec.what.to_lowercase().contains(&filter)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_spelling_is_its_own() {
        let mut seen = BTreeSet::new();
        for spec in COMMANDS {
            assert!(seen.insert(spec.name), "`{}` is listed twice", spec.name);
            for alias in spec.aliases {
                assert!(
                    seen.insert(alias),
                    "`{alias}` spells both `{}` and something else",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn a_command_is_found_by_its_name_or_by_a_spelling_of_it() {
        assert_eq!(find("open").map(|spec| spec.name), Some("open"));
        assert_eq!(find("o").map(|spec| spec.name), Some("open"));
        assert_eq!(find("show").map(|spec| spec.name), Some("open"));
        assert!(find("").is_none());
        assert!(find("pu").is_none());
        assert!(find("stat").is_none());
    }

    #[test]
    fn the_list_is_searched_by_what_a_command_does_as_well_as_by_its_name() {
        let named: Vec<&str> = matching("tag").map(|spec| spec.name).collect();
        assert_eq!(named, vec!["tags", "tag"]);

        let described: Vec<&str> = matching("remote").map(|spec| spec.name).collect();
        assert!(described.contains(&"push"), "{described:?}");
        assert!(described.contains(&"pull"), "{described:?}");

        assert_eq!(matching("").count(), COMMANDS.len());
        assert_eq!(matching("zzz").count(), 0);
    }

    #[test]
    fn what_a_command_takes_reads_as_one_line() {
        let open = find("open").expect("open");
        assert_eq!(open.usage(), "open <note>");
        let quit = find("quit").expect("quit");
        assert_eq!(quit.usage(), "quit");
    }
}
