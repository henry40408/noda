//! The query `noda search` takes: a few `field:value` terms, `OR` between the
//! alternatives, `-` in front of what must not match.
//!
//! Deliberately small: a query language compounds, so the grammar is fixed at
//! one shape and the shape is written down.
//!
//! ```text
//! query := group (' ' group)*        every group must match
//! group := term ('OR' term)*         any term in the group will do
//! term  := ['-'] [field ':'] value
//! field := tag | title | id | text
//! ```
//!
//! An AND of ORs — every query in conjunctive normal form, so parentheses buy
//! nothing. `(a AND b) OR (c AND d)` is unsayable; that is two searches, and
//! rare enough to be worth a grammar that fits in four lines.
//!
//! `OR` binds tighter than the space, so `budget tag:x OR tag:y` is
//! `budget AND (tag:x OR tag:y)` — what somebody listing alternatives for one
//! field expects, not what boolean algebra would give.
//!
//! A leading `-` is always a negation, so `text:--flag` is how a term starting
//! with one is written. The field prefix is the escape, which is why there is no
//! quoting: the shell already quotes and one token is one term.
//!
//! Every field matches the way noda already matches that thing — a tag whole, an
//! id by folded prefix, text and titles by case-insensitive substring, because
//! splitting on spaces finds nothing in a language that does not use them.

use crate::note::{self, Note};
use crate::{Error, Result};

/// One line of typing split as a shell would split it: on whitespace, but not
/// inside quotes.
///
/// "The shell's quoting is the only quoting" holds at a command line and nowhere
/// else — the browser's `/`, its `:` prompt and the listing's search box are
/// single fields with no shell in front. Doing it here keeps the three from
/// doing it three ways; they already grew this bug once.
///
/// Concretely: a tag may contain a space, so `tag:"24.04 Dark patterns"` has to
/// survive as one token or the tag is unreachable from the screen showing it.
///
/// Either quote character. An unclosed one runs to the end rather than failing —
/// the line is still being typed.
pub fn split(text: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut piece = String::new();
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(open) if c == open => quote = None,
            None if c == '"' || c == '\'' => quote = Some(c),
            // Inside the quotes a space is part of the value.
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

/// The query narrowing a listing to one tag, written so `split` gives it back
/// whole. Unquoted, `tag:24.04 Dark patterns` is three and-ed terms that find
/// nothing — and every screen listing tags offers to filter by one, so the
/// answer lives here rather than in each of them.
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
    /// The same grouping in the words it was typed in, because a `Term` cannot
    /// be shown — it has already thrown away the `tag:`, the quotes and the `-`
    /// that have to go back on the screen.
    ///
    /// A second copy, so it is filled by the one loop that does the grouping:
    /// two functions splitting on `OR` is how they come to disagree.
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
    /// One token per argument, so `noda search "title:Q3 budget" tag:work`
    /// arrives as two terms and no escape syntax has to be invented.
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

    /// The grouping, said back in the tokens it was written with: the outer list
    /// is and-ed, each inner list or-ed.
    ///
    /// `OR` binding tighter than a space is the opposite of what most search
    /// boxes do, and a caller that can draw the grouping answers that without a
    /// manual. Tokens come back exactly as given, so what is shown is the
    /// reader's own text.
    pub fn grouping(&self) -> &[Vec<String>] {
        &self.said
    }

    /// Whether a note satisfies every group.
    pub fn matches(&self, id: &str, note: &Note) -> bool {
        self.groups
            .iter()
            .all(|group| group.iter().any(|term| term.matches(id, note)))
    }

    /// For quoting the line a hit was on. A `tag:` or `id:` matched something
    /// outside the body, so there is nothing there to point at.
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

        // First colon only: `title:Rust: a tour` looks for `Rust: a tour`.
        let (field, value) = match rest.split_once(':') {
            Some((name, value)) => match Field::parse(name) {
                Some(field) => (field, value),
                // `https://example.com` searches for a URL, so only the known
                // names count as fields.
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

    /// Three fields stand in for argv, and this is the one account of them.
    #[test]
    fn a_field_splits_the_way_a_shell_does() {
        assert_eq!(split("+work -q3"), vec!["+work", "-q3"]);
        assert_eq!(split("  +work   "), vec!["+work"]);
        assert!(split("   ").is_empty());
        // Quotes around the name, not the whole piece: the `-` says remove.
        assert_eq!(split("-'a b' +c"), vec!["-a b", "+c"]);
        assert_eq!(split("-\"a b\""), vec!["-a b"]);
        // Quoted whole, which is what a hand used to a shell may well type.
        assert_eq!(split("\"-a b\""), vec!["-a b"]);
        // Still being typed, so an unclosed quote takes the rest.
        assert_eq!(split("-\"a b"), vec!["-a b"]);
        // A tag with a space survives as one term.
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

    /// Groups are and-ed and `OR` never reaches across a space.
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

    /// `grouping` has to hand back the shape `matches` applies, or a page draws
    /// one grouping while the notes were narrowed by another.
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

    /// Tokens come back as written, `-` and quotes and all — `Term` has thrown
    /// away everything needed to write them again.
    #[test]
    fn the_grouping_keeps_the_words_that_were_typed() {
        // Through `split`, the road a browser's query takes.
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

    /// A language without spaces is searched by substring or not at all.
    #[test]
    fn cjk_is_matched_by_substring() {
        let note = a_note("會議記錄", &["工作"], "討論第三季預算\n");
        assert!(query("第三季預算").matches("k3f9m2p1", &note));
        assert!(query("title:會議").matches("k3f9m2p1", &note));
        assert!(query("tag:工作").matches("k3f9m2p1", &note));
    }

    /// Lowercase `or` is the English word, or it would be unsearchable.
    #[test]
    fn only_an_uppercase_or_is_the_operator() {
        let note = a_note("x", &[], "this or that\n");
        assert!(query("or").matches("k3f9m2p1", &note));
        assert!(!query("or").matches("k3f9m2p1", &a_note("x", &[], "neither\n")));
    }

    /// A leading `-` is always a negation, so the field prefix is the escape.
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

    /// A `tag:` match points at nothing in the body to quote.
    #[test]
    fn only_text_terms_are_worth_quoting_a_line_for() {
        let q = query("budget title:meeting tag:work -hiring");
        assert_eq!(q.excerpt_terms(), ["budget", "meeting"]);
    }
}
