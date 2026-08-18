//! The query `noda search` takes: a few `field:value` terms, `OR` between the
//! alternatives, `-` in front of what must not match.
//!
//! Deliberately small. A query language compounds — once `tag:` exists there is
//! a case for `OR`, then for parentheses, then for ranges and regex — so the
//! grammar is fixed at one shape and the shape is written down:
//!
//! ```text
//! query := group (' ' group)*        every group must match
//! group := term ('OR' term)*         any term in the group will do
//! term  := ['-'] [field ':'] value
//! field := tag | title | id | text
//! ```
//!
//! Which is to say: an AND of ORs, and nothing else. That is every query in
//! conjunctive normal form, so parentheses buy nothing — `a OR b c OR d` already
//! reads as `(a OR b) AND (c OR d)`. What it cannot say is `(a AND b) OR (c AND
//! d)`; that is two searches, and it is rare enough to be worth the grammar
//! staying explainable in four lines.
//!
//! `OR` binds tighter than the space between groups, so `budget tag:x OR tag:y`
//! is `budget AND (tag:x OR tag:y)` — the reading someone writing a list of
//! alternatives for one field expects, rather than the one boolean algebra would
//! give.
//!
//! A leading `-` is always a negation, so a term that genuinely begins with one
//! is written `text:--flag`. The field prefix is the escape, which is why the
//! grammar needs no quoting of its own: the shell already quotes, and one token
//! per argument is one term.
//!
//! Every field matches the way noda already matches that thing: a tag whole,
//! because that is how `ls --tag` compares one; an id by prefix and folded,
//! because that is how `noda show k3f9` finds a note; text and titles by
//! case-insensitive substring, because splitting on spaces finds nothing at all
//! in a language that does not use them.

use crate::note::{self, Note};
use crate::{Error, Result};

/// One line of typing split into the tokens `parse` wants, the way a shell would
/// split it: on whitespace, but not inside quotes.
///
/// The module comment above says one token per argument *so that the shell's
/// quoting is the only quoting*. That holds at a command line and nowhere else:
/// every other place a query is typed — the browser's `/`, its `:` prompt, the
/// web listing's search box — is a single field with no shell in front of it, so
/// each has to do the shell's half of the job as well. Doing it here is what
/// keeps them from doing it three different ways; the browser's fields already
/// grew this bug once, and were fixed one field at a time.
///
/// What it buys concretely: a tag may contain a space — `24.04 Dark patterns` is
/// the sort of thing a `TiddlyWiki` import leaves behind — so `tag:"24.04 Dark
/// patterns"` has to survive as one token or the tag is unreachable from the one
/// screen showing it.
///
/// Either quote character, because both are what the hands reach for. An
/// unclosed quote runs to the end rather than being an error: the line is being
/// typed, and the character that would close it is usually the next one.
pub fn split(text: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut piece = String::new();
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(open) if c == open => quote = None,
            None if c == '"' || c == '\'' => quote = Some(c),
            // Only outside the quotes does a space end a piece. Inside them it
            // is part of the value, which is the whole point.
            None if c.is_whitespace() => {
                if !piece.is_empty() {
                    pieces.push(std::mem::take(&mut piece));
                }
            }
            _ => piece.push(c),
        }
    }
    if !piece.is_empty() {
        pieces.push(piece);
    }
    pieces
}

/// The query that narrows a listing to one tag, written so that `split` gives it
/// back whole.
///
/// The inverse of the function above, and here for the same reason: every screen
/// that lists tags offers to filter by one, and a tag may contain a space —
/// `tag:24.04 Dark patterns` is three terms and-ed together, which finds nothing
/// at all. The browser's tag screen worked this out once on its own; the web's
/// asks here instead, so there is one answer to be wrong.
pub fn scoped(tag: &str) -> String {
    if tag.contains(char::is_whitespace) {
        format!("tag:\"{tag}\"")
    } else {
        format!("tag:{tag}")
    }
}

/// A parsed query: groups that must all match, each satisfied by any one of its
/// terms.
pub struct Query {
    groups: Vec<Vec<Term>>,
    /// The same grouping, in the words it was typed in.
    ///
    /// Kept because something wanted to *show* the grouping rather than apply
    /// it, and a `Term` cannot be shown: it is what the token means, with the
    /// `tag:`, the quotes and the leading `-` already read and thrown away.
    /// What has to go back on a screen is what the reader put there.
    ///
    /// It is a second copy, which is a thing worth being uneasy about, so it is
    /// filled in the one loop that does the grouping rather than by a second
    /// pass over the tokens. Two functions splitting on `OR` is how they come
    /// to disagree; one loop appending to both cannot.
    said: Vec<Vec<String>>,
}

struct Term {
    field: Field,
    value: String,
    /// Whether the term must *not* match — a leading `-`.
    negated: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Tag,
    Title,
    Id,
    /// The title, the tags and the body together: what a bare word searches.
    Text,
}

/// The word that joins alternatives. Uppercase only, so that searching for the
/// English word `or` stays possible.
const OR: &str = "OR";

impl Query {
    /// Parses one token per argument, so the shell's own quoting is the only
    /// quoting there is: `noda search "title:Q3 budget" tag:work` arrives here
    /// as two terms, and no escape syntax has to be invented or explained.
    pub fn parse(tokens: &[String]) -> Result<Query> {
        let mut groups: Vec<Vec<Term>> = Vec::new();
        let mut said: Vec<Vec<String>> = Vec::new();
        let mut expecting = false;

        for token in tokens {
            if token.trim().is_empty() {
                continue;
            }
            if token == OR {
                if groups.is_empty() || expecting {
                    return Err(Error::msg("`OR` needs a term on both sides"));
                }
                expecting = true;
                continue;
            }
            let term = Term::parse(token)?;
            match groups.last_mut() {
                Some(group) if expecting => group.push(term),
                _ => groups.push(vec![term]),
            }
            match said.last_mut() {
                Some(group) if expecting => group.push(token.clone()),
                _ => said.push(vec![token.clone()]),
            }
            expecting = false;
        }

        if expecting {
            return Err(Error::msg("`OR` needs a term on both sides"));
        }
        if groups.is_empty() {
            return Err(Error::msg("search needs something to look for"));
        }
        Ok(Query { groups, said })
    }

    /// The grouping this query arrived at, said back in the tokens it was
    /// written with.
    ///
    /// `a OR b c` is `(a OR b) AND c`, and that precedence is the one thing
    /// about this grammar that gets read wrong — `OR` binding tighter than a
    /// space is the opposite of what most search boxes do. A caller that can
    /// draw the grouping can answer that without a manual, and this is what it
    /// draws: the outer list is and-ed, each inner list is or-ed.
    ///
    /// Every token comes back exactly as it was given, so whatever is shown is
    /// the reader's own text and not this parser's opinion of it.
    pub fn grouping(&self) -> &[Vec<String>] {
        &self.said
    }

    /// Whether a note satisfies every group.
    pub fn matches(&self, id: &str, note: &Note) -> bool {
        self.groups
            .iter()
            .all(|group| group.iter().any(|term| term.matches(id, note)))
    }

    /// The text terms, for quoting the line a hit was found on. A `tag:` or an
    /// `id:` matched something that is not in the body, so there is nothing
    /// there to point at.
    pub fn excerpt_terms(&self) -> Vec<String> {
        self.groups
            .iter()
            .flatten()
            .filter(|term| !term.negated && matches!(term.field, Field::Text | Field::Title))
            .map(|term| term.value.to_lowercase())
            .collect()
    }
}

impl Term {
    fn parse(token: &str) -> Result<Term> {
        let (negated, rest) = match token.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, token),
        };
        if rest.is_empty() {
            return Err(Error::msg("`-` needs something after it"));
        }

        // Split at the first colon only: a title may well contain one, and
        // `title:Rust: a tour` should look for `Rust: a tour`.
        let (field, value) = match rest.split_once(':') {
            Some((name, value)) => match Field::parse(name) {
                Some(field) => (field, value),
                // `https://example.com` is a search for a URL, not a query for
                // a field nobody has heard of. Only the known names are fields.
                None => (Field::Text, rest),
            },
            None => (Field::Text, rest),
        };
        if value.is_empty() {
            return Err(Error::msg(format!("`{token}` has nothing to look for")));
        }
        Ok(Term {
            field,
            value: value.to_string(),
            negated,
        })
    }

    fn matches(&self, id: &str, note: &Note) -> bool {
        let found = match self.field {
            Field::Tag => note.tags.iter().any(|tag| tag == &self.value),
            Field::Id => note::normalize_id(id).starts_with(&note::normalize_id(&self.value)),
            Field::Title => contains_ignoring_case(&note.title, &self.value),
            Field::Text => {
                contains_ignoring_case(&note.title, &self.value)
                    || contains_ignoring_case(&note.body, &self.value)
                    || note
                        .tags
                        .iter()
                        .any(|tag| contains_ignoring_case(tag, &self.value))
            }
        };
        found != self.negated
    }
}

impl Field {
    fn parse(name: &str) -> Option<Field> {
        match name {
            "tag" => Some(Field::Tag),
            "title" => Some(Field::Title),
            "id" => Some(Field::Id),
            "text" => Some(Field::Text),
            _ => None,
        }
    }
}

fn contains_ignoring_case(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Written for the browser's prompts, kept when the splitter moved here: a
    /// field standing in for argv is now three fields, and this is the one
    /// account of what they all do.
    #[test]
    fn a_field_splits_the_way_a_shell_does() {
        assert_eq!(split("+work -q3"), vec!["+work", "-q3"]);
        assert_eq!(split("  +work   "), vec!["+work"]);
        assert!(split("   ").is_empty());
        // Either quote, and the quotes around the name rather than around the
        // whole piece — the `-` in front of them is what says remove.
        assert_eq!(split("-'a b' +c"), vec!["-a b", "+c"]);
        assert_eq!(split("-\"a b\""), vec!["-a b"]);
        // Quoted whole, which is what a hand used to a shell may well type.
        assert_eq!(split("\"-a b\""), vec!["-a b"]);
        // Half-typed: the line is still being written, so the quote that has not
        // been closed yet takes the rest rather than failing.
        assert_eq!(split("-\"a b"), vec!["-a b"]);
        // What the web listing's box is for: a tag with a space in it survives
        // as one term, so it can be filtered by from the screen showing it.
        assert_eq!(
            split("tag:\"24.04 Dark patterns\" budget"),
            vec!["tag:24.04 Dark patterns", "budget"]
        );
    }

    fn query(text: &str) -> Query {
        let tokens: Vec<String> = text.split(' ').map(str::to_string).collect();
        Query::parse(&tokens).unwrap()
    }

    fn a_note(title: &str, tags: &[&str], body: &str) -> Note {
        Note {
            title: title.to_string(),
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
            created: None,
            updated: None,
            extra: Vec::new(),
            body: body.to_string(),
        }
    }

    #[test]
    fn a_bare_word_searches_the_title_the_tags_and_the_body() {
        let note = a_note("Meeting Notes", &["work"], "the Q3 budget\n");
        assert!(query("budget").matches("k3f9m2p1", &note));
        assert!(query("meeting").matches("k3f9m2p1", &note), "and the title");
        assert!(query("work").matches("k3f9m2p1", &note), "and the tags");
        assert!(!query("hiring").matches("k3f9m2p1", &note));
    }

    #[test]
    fn separate_terms_must_all_match() {
        let note = a_note("Meeting Notes", &[], "the Q3 budget\n");
        assert!(query("q3 budget").matches("k3f9m2p1", &note));
        assert!(!query("q3 hiring").matches("k3f9m2p1", &note));
    }

    #[test]
    fn or_is_satisfied_by_either_side() {
        let alpha = a_note("Alpha", &["work"], "x\n");
        let beta = a_note("Beta", &["home"], "x\n");
        let other = a_note("Gamma", &["q3"], "x\n");
        let q = query("tag:work OR tag:home");
        assert!(q.matches("k3f9m2p1", &alpha));
        assert!(q.matches("k3f9m2p1", &beta));
        assert!(!q.matches("k3f9m2p1", &other));
    }

    /// The precedence that makes parentheses unnecessary: groups are joined
    /// with AND, and `OR` never reaches across a space.
    #[test]
    fn or_binds_tighter_than_the_space_between_groups() {
        let both = a_note("Alpha", &["work", "q3"], "budget\n");
        let wrong_tag = a_note("Alpha", &["home"], "budget\n");
        let wrong_body = a_note("Alpha", &["work"], "hiring\n");

        let q = query("budget tag:work OR tag:q3");
        assert!(q.matches("k3f9m2p1", &both));
        assert!(
            !q.matches("k3f9m2p1", &wrong_tag),
            "the bare term is ANDed, not swallowed by the OR"
        );
        assert!(!q.matches("k3f9m2p1", &wrong_body));
    }

    /// The same precedence, read off the other end: what `grouping` hands back
    /// has to be the shape `matches` applies, or a page could draw one grouping
    /// while the notes were narrowed by another.
    #[test]
    fn the_grouping_shown_is_the_grouping_applied() {
        assert_eq!(
            query("budget tag:work OR tag:q3").grouping(),
            [
                vec!["budget".to_string()],
                vec!["tag:work".to_string(), "tag:q3".to_string()]
            ]
        );
        assert_eq!(
            query("tag:a OR tag:b tag:c OR tag:d").grouping(),
            [
                vec!["tag:a".to_string(), "tag:b".to_string()],
                vec!["tag:c".to_string(), "tag:d".to_string()]
            ]
        );
    }

    /// The tokens come back as they were written, `-` and quotes and all. A
    /// caller drawing them is drawing the reader's own line, and `Term` has
    /// already thrown away everything needed to write it again.
    #[test]
    fn the_grouping_keeps_the_words_that_were_typed() {
        // Through `split`, because that is the road a browser's query takes:
        // one line typed into one field. What comes back is what was in it,
        // leading `-` and all — `Term` has already thrown that away.
        let typed = split("-tag:archived title:\"Q3 budget\"");
        assert_eq!(
            Query::parse(&typed).unwrap().grouping(),
            [
                vec!["-tag:archived".to_string()],
                vec!["title:Q3 budget".to_string()]
            ]
        );
    }

    #[test]
    fn two_or_groups_side_by_side_are_anded() {
        let q = query("tag:a OR tag:b tag:c OR tag:d");
        assert!(q.matches("k3f9m2p1", &a_note("x", &["a", "d"], "")));
        assert!(q.matches("k3f9m2p1", &a_note("x", &["b", "c"], "")));
        assert!(!q.matches("k3f9m2p1", &a_note("x", &["a", "b"], "")));
    }

    #[test]
    fn a_negated_term_must_not_match() {
        let q = query("budget -tag:archived");
        assert!(q.matches("k3f9m2p1", &a_note("x", &["work"], "budget\n")));
        assert!(!q.matches("k3f9m2p1", &a_note("x", &["archived"], "budget\n")));
    }

    #[test]
    fn each_field_matches_the_way_noda_matches_that_thing() {
        let note = a_note("Meeting Notes", &["work"], "body\n");
        // A tag whole, the way `ls --tag` compares one.
        assert!(query("tag:work").matches("k3f9m2p1", &note));
        assert!(!query("tag:wor").matches("k3f9m2p1", &note));
        // A title by substring, and case does not matter.
        assert!(query("title:meeting").matches("k3f9m2p1", &note));
        assert!(
            !query("title:body").matches("k3f9m2p1", &note),
            "not the body"
        );
        // An id by prefix, folded, the way `noda show k3f9` finds a note.
        assert!(query("id:k3f9").matches("k3f9m2p1", &note));
        assert!(query("id:K3F9").matches("k3f9m2p1", &note));
        assert!(!query("id:q7x2").matches("k3f9m2p1", &note));
    }

    /// No tokenizer anywhere: a language without spaces has to be searched by
    /// substring or it is not searched at all.
    #[test]
    fn cjk_is_matched_by_substring() {
        let note = a_note("會議記錄", &["工作"], "討論第三季預算\n");
        assert!(query("第三季預算").matches("k3f9m2p1", &note));
        assert!(query("title:會議").matches("k3f9m2p1", &note));
        assert!(query("tag:工作").matches("k3f9m2p1", &note));
    }

    /// Lowercase `or` is the English word, not the operator — otherwise it would
    /// be unsearchable.
    #[test]
    fn only_an_uppercase_or_is_the_operator() {
        let note = a_note("x", &[], "this or that\n");
        assert!(query("or").matches("k3f9m2p1", &note));
        assert!(!query("or").matches("k3f9m2p1", &a_note("x", &[], "neither\n")));
    }

    /// A leading `-` is always a negation, so the field prefix is how a term
    /// that genuinely starts with one gets searched for.
    #[test]
    fn text_is_the_way_to_look_for_something_starting_with_a_hyphen() {
        let note = a_note("x", &[], "a --flag in the body\n");
        let plain = a_note("x", &[], "nothing like it\n");
        assert!(query("text:--flag").matches("k3f9m2p1", &note));
        assert!(!query("text:--flag").matches("k3f9m2p1", &plain));
        assert!(
            query("--flag").matches("k3f9m2p1", &plain),
            "without the prefix it reads as `not -flag`, which the plain note satisfies"
        );
    }

    /// A colon is ordinary punctuation until it follows a field's name.
    #[test]
    fn an_unknown_prefix_is_text_rather_than_a_field() {
        let note = a_note("x", &[], "see https://example.com/x\n");
        assert!(query("https://example.com/x").matches("k3f9m2p1", &note));
    }

    #[test]
    fn a_value_keeps_every_colon_after_the_first() {
        let note = a_note("Rust: a tour", &[], "body\n");
        let q = Query::parse(&["title:Rust: a tour".to_string()]).unwrap();
        assert!(q.matches("k3f9m2p1", &note));
    }

    #[test]
    fn a_query_that_says_nothing_is_refused() {
        for bad in [
            vec![],
            vec!["OR".to_string()],
            vec!["a".to_string(), "OR".to_string()],
            vec!["OR".to_string(), "a".to_string()],
            vec!["-".to_string()],
            vec!["tag:".to_string()],
        ] {
            assert!(Query::parse(&bad).is_err(), "{bad:?} should be refused");
        }
    }

    /// A `tag:` match points at nothing in the body, so it must not be quoted as
    /// though it were found there.
    #[test]
    fn only_text_terms_are_worth_quoting_a_line_for() {
        let q = query("budget title:meeting tag:work -hiring");
        assert_eq!(q.excerpt_terms(), ["budget", "meeting"]);
    }
}
