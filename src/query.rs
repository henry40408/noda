//! The query `noda search` takes, deliberately fixed at one small shape:
//!
//! ```text
//! query := group (' ' group)*        every group must match
//! group := term ('OR' term)*         any term in the group will do
//! term  := ['-'] [field ':'] value
//! field := tag | title | id | pinned | text
//! ```
//!
//! An AND of ORs, so there are no parentheses; `(a AND b) OR (c AND d)` is two
//! searches. `OR` binds tighter than the space: `budget tag:x OR tag:y` is
//! `budget AND (tag:x OR tag:y)`, what someone listing alternatives expects.
//!
//! A leading `-` is always a negation; the field prefix is the escape
//! (`text:--flag`). One token is one term, so the grammar has no quoting.
//!
//! A tag matches whole, an id by folded prefix, text and titles by
//! case-insensitive substring (splitting on spaces finds nothing in CJK).
//! `pinned:` takes only `true` or `false`.

use crate::note::{self, Note};
use crate::{Error, Result};

/// One line split as a shell would: on whitespace, not inside `"` or `'`. For
/// the TUI and web inputs with no shell in front, in one place so they cannot
/// diverge. A tag may contain a space, so `tag:"24.04 Dark patterns"` must stay
/// one token. An unclosed quote runs to the end: the line is still being typed.
pub fn split(text: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut piece = String::new();
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(open) if c == open => quote = None,
            None if c == '"' || c == '\'' => quote = Some(c),
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

/// The query narrowing a listing to one tag, quoted when needed so `split`
/// gives it back as one term.
pub fn scoped(tag: &str) -> String {
    if tag.contains(char::is_whitespace) {
        format!("tag:\"{tag}\"")
    } else {
        format!("tag:{tag}")
    }
}

pub struct Query {
    groups: Vec<Vec<Term>>,
    /// The same grouping as typed, for display (a `Term` has dropped the field
    /// and the `-`). Filled by the same loop so the two cannot disagree.
    said: Vec<Vec<String>>,
}

struct Term {
    field: Field,
    value: String,
    negated: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Tag,
    Title,
    Id,
    /// `true` or `false` only: compared rather than searched.
    Pinned,
    /// Title, tags and body: what a bare word searches.
    Text,
}

/// Uppercase only, so the English word `or` stays searchable.
const OR: &str = "OR";

impl Query {
    /// One token per term: `noda search "title:Q3 budget" tag:work` is two.
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

    /// The grouping in the tokens as given: the outer list and-ed, each inner
    /// list or-ed. Shown because `OR` binding tighter than a space surprises.
    pub fn grouping(&self) -> &[Vec<String>] {
        &self.said
    }

    pub fn matches(&self, id: &str, note: &Note) -> bool {
        self.groups
            .iter()
            .all(|group| group.iter().any(|term| term.matches(id, note)))
    }

    /// The terms worth quoting a matching line for: positive `text` and `title`.
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
                // Only known names are fields, so `https://…` is text.
                None => (Field::Text, rest),
            },
            None => (Field::Text, rest),
        };
        if value.is_empty() {
            return Err(Error::msg(format!("`{token}` has nothing to look for")));
        }
        // Other fields find nothing on nonsense; `pinned:ture` would silently
        // read as `pinned:false`, so it is refused.
        if field == Field::Pinned && !matches!(value, "true" | "false") {
            return Err(Error::msg(format!(
                "`pinned:` takes `true` or `false`, not `{value}`"
            )));
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
            Field::Pinned => note.is_pinned() == (self.value == "true"),
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
            "pinned" => Some(Field::Pinned),
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

    #[test]
    fn a_field_splits_the_way_a_shell_does() {
        assert_eq!(split("+work -q3"), vec!["+work", "-q3"]);
        assert_eq!(split("  +work   "), vec!["+work"]);
        assert!(split("   ").is_empty());
        assert_eq!(split("-'a b' +c"), vec!["-a b", "+c"]);
        assert_eq!(split("-\"a b\""), vec!["-a b"]);
        assert_eq!(split("\"-a b\""), vec!["-a b"]);
        assert_eq!(split("-\"a b"), vec!["-a b"]);
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
            pinned: None,
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
    fn pinned_asks_for_one_of_two_states_and_the_negation_asks_for_the_other() {
        let mut pinned = a_note("Alpha", &[], "x\n");
        pinned.pinned = Some(note::PINNED.to_string());
        let loose = a_note("Beta", &[], "x\n");

        assert!(query("pinned:true").matches("k3f9m2p1", &pinned));
        assert!(!query("pinned:true").matches("k3f9m2p1", &loose));
        assert!(query("pinned:false").matches("k3f9m2p1", &loose));
        assert!(!query("pinned:false").matches("k3f9m2p1", &pinned));
        assert!(query("-pinned:true").matches("k3f9m2p1", &loose));

        // `is_pinned`'s answer is compared, not the field's presence.
        let mut odd = a_note("Gamma", &[], "x\n");
        odd.pinned = Some("yes".to_string());
        assert!(query("pinned:false").matches("k3f9m2p1", &odd));
    }

    #[test]
    fn pinned_refuses_a_value_that_is_not_one_of_the_two() {
        let tokens = |text: &str| vec![text.to_string()];
        assert!(Query::parse(&tokens("pinned:ture")).is_err());
        assert!(Query::parse(&tokens("pinned:yes")).is_err());
        assert!(Query::parse(&tokens("-pinned:1")).is_err());
        assert!(Query::parse(&tokens("pinned:true")).is_ok());
        assert!(Query::parse(&tokens("pinnedness")).is_ok());
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

    #[test]
    fn the_grouping_keeps_the_words_that_were_typed() {
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
        assert!(query("tag:work").matches("k3f9m2p1", &note));
        assert!(!query("tag:wor").matches("k3f9m2p1", &note));
        assert!(query("title:meeting").matches("k3f9m2p1", &note));
        assert!(
            !query("title:body").matches("k3f9m2p1", &note),
            "not the body"
        );
        assert!(query("id:k3f9").matches("k3f9m2p1", &note));
        assert!(query("id:K3F9").matches("k3f9m2p1", &note));
        assert!(!query("id:q7x2").matches("k3f9m2p1", &note));
    }

    #[test]
    fn cjk_is_matched_by_substring() {
        let note = a_note("會議記錄", &["工作"], "討論第三季預算\n");
        assert!(query("第三季預算").matches("k3f9m2p1", &note));
        assert!(query("title:會議").matches("k3f9m2p1", &note));
        assert!(query("tag:工作").matches("k3f9m2p1", &note));
    }

    #[test]
    fn only_an_uppercase_or_is_the_operator() {
        let note = a_note("x", &[], "this or that\n");
        assert!(query("or").matches("k3f9m2p1", &note));
        assert!(!query("or").matches("k3f9m2p1", &a_note("x", &[], "neither\n")));
    }

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

    #[test]
    fn only_text_terms_are_worth_quoting_a_line_for() {
        let q = query("budget title:meeting tag:work -hiring");
        assert_eq!(q.excerpt_terms(), ["budget", "meeting"]);
    }
}
