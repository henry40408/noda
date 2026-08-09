//! What `:` accepts.
//!
//! The names are noda's own subcommand names. A browser that invented a second
//! vocabulary for the same commands would be a second thing to learn, and the
//! whole reason for a prompt here is that the vocabulary already exists — thirty
//! subcommands, of which a key can only ever reach the handful worth a letter.
//!
//! The table is data rather than a `match` arm apiece so that it can be read
//! back: `Ctrl-a` shows it, filtered as you type, and the help card counts it.
//! What each one *means* is in `app`, next to the state it moves, because that
//! is the part that needs the session in hand.

/// One command, as it is typed and as it is listed.
pub struct Spec {
    /// The name this command is known by, which is the noda subcommand's.
    pub name: &'static str,
    /// Other spellings, shortest first. A prefix is not enough on its own —
    /// `s` would have to choose between `status`, `snapshot` and `sync`, and a
    /// prompt that guessed would eventually guess wrong about a `push`.
    pub aliases: &'static [&'static str],
    /// What it takes after the name, in the notation the CLI's own help uses:
    /// `<required>`, `[optional]`, `...` for more than one.
    pub takes: &'static str,
    /// One line, in the words the subcommand's own help uses.
    pub what: &'static str,
}

impl Spec {
    /// The name and what it takes, as one thing to show in a list.
    pub fn usage(&self) -> String {
        if self.takes.is_empty() {
            self.name.to_string()
        } else {
            format!("{} {}", self.name, self.takes)
        }
    }
}

/// Ordered the way somebody reading down the list would want them: getting
/// about, then changing a note, then the notebook as a whole, then leaving.
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
    // No note may be named. A delete is the one thing here that asks a
    // question, and the question is only worth asking about a note you can see
    // — naming one would put the answer on a screen showing something else.
    Spec {
        name: "rm",
        aliases: &[],
        takes: "",
        what: "delete the note on screen, after a y",
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

/// The command a name spells, by its own name or by one of its other spellings.
///
/// Exact only. A prompt that completed a prefix would have to choose between
/// `push` and `pull` on `pu`, and the wrong choice there is a network call you
/// did not ask for.
pub fn find(name: &str) -> Option<&'static Spec> {
    COMMANDS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
}

/// The commands whose name, spelling or description contains what has been
/// typed, in table order.
///
/// The description is searched as well as the name, because the list is for
/// somebody who knows what they want to do and not what it is called — `remote`
/// finds `push` and `pull` that way.
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
        // Not a prefix: `pu` spells both `push` and `pull`, and guessing there
        // is a network call nobody asked for.
        assert!(find("pu").is_none());
        assert!(find("stat").is_none());
    }

    #[test]
    fn the_list_is_searched_by_what_a_command_does_as_well_as_by_its_name() {
        let named: Vec<&str> = matching("tag").map(|spec| spec.name).collect();
        assert_eq!(named, vec!["tag"]);

        // What somebody who wants to get their notes onto the remote would type,
        // knowing that and not the three names it goes by.
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
