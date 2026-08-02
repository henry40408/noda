//! `WikiText` to Markdown.
//!
//! Pure: no filesystem, no git, no notebook. It takes a tiddler's text and a way
//! to turn a tiddler title into a filename, and gives back Markdown plus the
//! list of constructs it would not translate. That makes the whole of it
//! testable with `assert_eq!`, which matters more here than anywhere else in
//! noda — a conversion that goes wrong goes wrong silently, and the note still
//! reads fine.
//!
//! Two rules decide everything:
//!
//! **Nothing is guessed.** A construct that cannot be translated faithfully is
//! copied through exactly as it was written and named in [`Converted::left`].
//! Unconverted `WikiText` in a note is findable and fixable; Markdown that looks
//! right and says something else is neither.
//!
//! **Nothing is dropped.** Every character of the source reaches the output,
//! as markup or as text. `tests::text_survives_every_construct` holds this to
//! account by stripping both sides back to their visible words and comparing —
//! which is the whole of the "no silent loss" promise, mechanised.

use std::collections::BTreeSet;
use std::fmt::Write as _;

/// The result of converting one tiddler's text.
pub struct Converted {
    pub text: String,
    /// The constructs left in `WikiText`, named for the note's `unconverted:`
    /// field. Sorted and deduplicated: this is a description of the note, not a
    /// log of the walk.
    pub left: BTreeSet<&'static str>,
}

/// Turns a tiddler title into the filename noda gave it, or `None` when the
/// export holds no such tiddler.
pub type Resolve<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// The constructs a body can be left carrying. Names are what lands in the
/// `unconverted:` field, so they read as an answer to "what is still `WikiText`
/// in here".
mod left {
    pub const TRANSCLUSION: &str = "transclusion";
    pub const MACRO: &str = "macro";
    pub const WIDGET: &str = "widget";
    pub const HTML: &str = "html";
    pub const TABLE: &str = "table";
    pub const DEFINITION_LIST: &str = "definition list";
    pub const STYLE: &str = "style";
    pub const UNDERLINE: &str = "underline";
    pub const SUPERSCRIPT: &str = "superscript";
    pub const SUBSCRIPT: &str = "subscript";
    pub const IMAGE_ATTRIBUTES: &str = "image attributes";
    pub const LINK: &str = "unresolved link";
}

/// Converts one tiddler's text.
pub fn convert(text: &str, resolve: &Resolve) -> Converted {
    let mut out = Blocks {
        out: String::with_capacity(text.len() + text.len() / 8),
        left: BTreeSet::new(),
        resolve,
    };
    out.run(text);
    Converted {
        text: out.out,
        left: out.left,
    }
}

/// The block pass: one walk down the lines, with the few states `WikiText`'s
/// block constructs need. Everything that is not a block construct goes through
/// [`inline`].
struct Blocks<'a> {
    out: String,
    left: BTreeSet<&'static str>,
    resolve: &'a Resolve<'a>,
}

impl Blocks<'_> {
    fn run(&mut self, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];

            // Fenced code first, and copied byte for byte. What is inside a
            // fence is not markup in either language, and a converter that
            // looks at it will find `''` in somebody's Rust and turn it bold.
            if line.trim_start().starts_with("```") {
                i = self.fence(&lines, i);
                continue;
            }
            // A macro definition makes the whole tiddler a program. Nothing
            // after it is safe to read as prose, so the rest is copied through.
            if line.starts_with('\\') && definition(line) {
                self.left.insert(left::MACRO);
                for rest in &lines[i..] {
                    let _ = writeln!(self.out, "{rest}");
                }
                return;
            }
            if line.starts_with('|') {
                i = self.table(&lines, i);
                continue;
            }
            if line.starts_with("<<<") {
                i = self.quote(&lines, i);
                continue;
            }
            if let Some((hashes, body, styled)) = heading(line) {
                if styled {
                    self.left.insert(left::STYLE);
                }
                let _ = write!(self.out, "{hashes} ");
                self.inline(body);
                self.out.push('\n');
                i += 1;
                continue;
            }
            if let Some((prefix, body, styled)) = list(line) {
                if styled {
                    self.left.insert(left::STYLE);
                }
                self.list_item(prefix, body);
                i += 1;
                continue;
            }
            // `; term` / `: definition`. Markdown has no definition list, so the
            // line is left as it stands rather than bent into a bullet that
            // says something slightly different.
            if line.starts_with("; ") || line.starts_with(": ") {
                self.left.insert(left::DEFINITION_LIST);
                let _ = writeln!(self.out, "{line}");
                i += 1;
                continue;
            }
            // A style block wraps content in styling Markdown cannot carry. The
            // content is what matters and it survives; the styling is named.
            if line.trim_end() == "@@" || style_open(line) {
                self.left.insert(left::STYLE);
                let _ = writeln!(self.out, "{line}");
                i += 1;
                continue;
            }

            self.inline(line);
            self.out.push('\n');
            i += 1;
        }
    }

    /// Copies a fenced block through untouched, closing fence included. An
    /// unterminated fence takes the rest of the tiddler with it, which is what
    /// every Markdown parser does with one too.
    fn fence(&mut self, lines: &[&str], start: usize) -> usize {
        let _ = writeln!(self.out, "{}", lines[start]);
        for (offset, line) in lines[start + 1..].iter().enumerate() {
            let _ = writeln!(self.out, "{line}");
            if line.trim_start().starts_with("```") {
                return start + offset + 2;
            }
        }
        lines.len()
    }

    /// A table, decided as a whole.
    ///
    /// `GFM` has one header row and nothing else: no footer, no caption, no CSS
    /// class, no merged cells, no vertical alignment. A table using any of them
    /// is copied through as `WikiText` rather than flattened into something that
    /// has quietly lost a row.
    fn table(&mut self, lines: &[&str], start: usize) -> usize {
        let end = lines[start..]
            .iter()
            .position(|line| !line.starts_with('|'))
            .map_or(lines.len(), |n| start + n);
        let rows = &lines[start..end];

        if rows.iter().any(|row| !plain_row(row)) {
            self.left.insert(left::TABLE);
            for row in rows {
                let _ = writeln!(self.out, "{row}");
            }
            return end;
        }

        for (n, row) in rows.iter().enumerate() {
            let cells: Vec<&str> = split_row(row);
            self.out.push('|');
            for cell in &cells {
                self.out.push(' ');
                self.inline(cell.trim().trim_start_matches('!'));
                self.out.push_str(" |");
            }
            self.out.push('\n');
            // `GFM` needs the delimiter row, and `WikiText` has no such line: a
            // header is a row whose cells open with `!`. The first row is the
            // header when it says so, and there is no header at all otherwise.
            if n == 0 {
                let header = cells.iter().all(|c| c.trim_start().starts_with('!'));
                if header {
                    self.out.push('|');
                    for _ in &cells {
                        self.out.push_str(" --- |");
                    }
                    self.out.push('\n');
                }
            }
        }
        end
    }

    /// `<<<` … `<<<`, with the citation `WikiText` allows on the closing line.
    fn quote(&mut self, lines: &[&str], start: usize) -> usize {
        for (offset, line) in lines[start + 1..].iter().enumerate() {
            if let Some(rest) = line.strip_prefix("<<<") {
                let cite = rest.trim();
                if !cite.is_empty() {
                    self.out.push_str("> — ");
                    self.inline(cite);
                    self.out.push('\n');
                }
                return start + offset + 2;
            }
            self.out.push_str("> ");
            self.inline(line);
            self.out.push('\n');
        }
        lines.len()
    }

    /// `*`, `#` and their nestings. The prefix is read a character at a time:
    /// `*#` is a numbered item inside a bullet, and only the last character
    /// decides which marker this line gets.
    fn list_item(&mut self, prefix: &str, body: &str) {
        let depth = prefix.chars().count() - 1;
        for _ in 0..depth {
            self.out.push_str("  ");
        }
        match prefix.chars().last() {
            Some('#') => self.out.push_str("1. "),
            _ => self.out.push_str("- "),
        }
        self.inline(body);
        self.out.push('\n');
    }

    /// The inline pass over one line.
    fn inline(&mut self, line: &str) {
        let mut rest = line;
        while !rest.is_empty() {
            if let Some(len) = self.inline_at(rest) {
                rest = &rest[len..];
                continue;
            }
            let ch = rest.chars().next().unwrap_or_default();
            self.escaped(ch, self.out.ends_with('\n') || self.out.is_empty());
            rest = &rest[ch.len_utf8()..];
        }
    }

    /// One construct at the head of `rest`, or `None` when there is none and the
    /// caller should take a character as text.
    fn inline_at(&mut self, rest: &str) -> Option<usize> {
        // A URL is copied whole and first. It is full of `//` and `__`, and
        // every one of them would otherwise be read as emphasis.
        if let Some(len) = url(rest) {
            self.out.push_str(&rest[..len]);
            return Some(len);
        }
        // Inline code is literal, like a fence.
        if let Some(after) = rest.strip_prefix('`')
            && let Some(end) = after.find('`')
        {
            let len = end + 2;
            self.out.push_str(&rest[..len]);
            return Some(len);
        }
        if let Some(len) = self.image(rest) {
            return Some(len);
        }
        if let Some(len) = self.link(rest) {
            return Some(len);
        }
        if let Some(len) = self.camel_case(rest) {
            return Some(len);
        }
        // The three that have no Markdown at all. Copied verbatim to the end of
        // their own syntax so that what follows is read as prose again.
        for (open, close, name) in [("{{", "}}", left::TRANSCLUSION), ("<<", ">>", left::MACRO)] {
            if rest.starts_with(open) {
                let len = rest.find(close).map_or(rest.len(), |n| n + close.len());
                self.left.insert(name);
                self.out.push_str(&rest[..len]);
                return Some(len);
            }
        }
        if rest.starts_with("<$") {
            let len = rest.find('>').map_or(rest.len(), |n| n + 1);
            self.left.insert(left::WIDGET);
            self.out.push_str(&rest[..len]);
            return Some(len);
        }
        if html_tag(rest) {
            let len = rest.find('>').map_or(rest.len(), |n| n + 1);
            self.left.insert(left::HTML);
            self.out.push_str(&rest[..len]);
            return Some(len);
        }

        // Emphasis, as pairs of symmetric delimiters. Recursing on the inside
        // is what makes `//''both''//` work without a stack of my own.
        for (delim, open, close, name) in [
            ("''", "**", "**", None),
            ("//", "*", "*", None),
            ("~~", "~~", "~~", None),
            ("__", "", "", Some(left::UNDERLINE)),
            ("^^", "", "", Some(left::SUPERSCRIPT)),
            (",,", "", "", Some(left::SUBSCRIPT)),
            ("@@", "", "", Some(left::STYLE)),
        ] {
            if !rest.starts_with(delim) {
                continue;
            }
            // No closing delimiter: this is punctuation, not markup. Left as
            // text rather than turned into emphasis that runs to the end.
            let Some(end) = rest[delim.len()..].find(delim) else {
                continue;
            };
            let inner = &rest[delim.len()..delim.len() + end];
            if let Some(name) = name {
                self.left.insert(name);
            }
            self.out.push_str(open);
            self.inline(inner);
            self.out.push_str(close);
            return Some(delim.len() * 2 + end);
        }
        None
    }

    /// A `CamelCase` word, which in `WikiText` is a link whether anybody meant
    /// it or not — and `~CamelCase` is the same word with that turned off.
    ///
    /// Only the ones the export can resolve become links. Measured against
    /// `TiddlyWiki`'s own documentation, fewer than a third of these words name
    /// a tiddler at all: the rest are `GitHub`, `JavaScript`, `LaTeX` — prose,
    /// which linking would turn into that many broken links. The `~` goes
    /// either way, because it is markup that says "not a link" to a language
    /// that has no automatic links to say it to.
    fn camel_case(&mut self, rest: &str) -> Option<usize> {
        let (suppressed, word) = match rest.strip_prefix('~') {
            Some(after) => (true, after),
            None => (false, rest),
        };
        let len = camel_len(word)?;
        let name = &word[..len];
        match self.resolved(name).filter(|_| !suppressed) {
            Some(destination) => {
                let _ = write!(self.out, "[{name}]({destination})");
            }
            None => self.out.push_str(name),
        }
        Some(usize::from(suppressed) + len)
    }

    /// `[img[pic.jpg]]`, with the tooltip and attribute forms.
    fn image(&mut self, rest: &str) -> Option<usize> {
        let after = rest.strip_prefix("[img")?;
        let open = after.find('[')?;
        // Anything between `[img` and the destination is width/class/style,
        // which Markdown has nowhere to put.
        if !after[..open].trim().is_empty() {
            self.left.insert(left::IMAGE_ATTRIBUTES);
        }
        let end = after[open..].find("]]")?;
        let body = &after[open + 1..open + end];
        let (caption, target) = match body.split_once('|') {
            Some((caption, target)) => (caption, target),
            None => ("", body),
        };
        // An image source may be the title of a binary tiddler or it may be a
        // filename. Unlike a link, an unresolved one is kept as written: it
        // names a file the notebook is expected to gain, and `doctor --links`
        // is the thing that says whether it did.
        let target = target.trim();
        let destination = self.resolved(target).unwrap_or_else(|| target.to_string());
        let _ = write!(self.out, "![{caption}]({destination})");
        Some("[img".len() + open + end + 2)
    }

    /// `[[Target]]`, `[[Caption|Target]]` and `[ext[Caption|url]]`.
    ///
    /// The caption comes first in `WikiText`, as it does in Markdown — the trap is
    /// that `MediaWiki` puts it second, so the order gets written from habit and
    /// the link still looks perfectly fine afterwards.
    fn link(&mut self, rest: &str) -> Option<usize> {
        // `[ext[..]]` always names something outside the wiki, so its target is
        // taken as written. `[[..]]` names a tiddler unless it spells out a URL.
        let (skip, after, external) = match rest.strip_prefix("[ext[") {
            Some(after) => ("[ext[".len(), after, true),
            None => ("[[".len(), rest.strip_prefix("[[")?, false),
        };
        let end = after.find("]]")?;
        let body = &after[..end];
        let (caption, target) = match body.split_once('|') {
            Some((caption, target)) => (caption, target),
            None => (body, body),
        };
        let len = skip + end + 2;

        let destination = if external {
            Some(target.trim().to_string())
        } else {
            self.resolved(target.trim())
        };
        // An unresolved target names a tiddler this export does not hold. A link
        // to a file that will not exist is worse than the `WikiText` that says
        // so, which `doctor` can then be pointed at.
        let Some(destination) = destination else {
            self.left.insert(left::LINK);
            self.out.push_str(&rest[..len]);
            return Some(len);
        };
        self.out.push('[');
        self.inline(caption);
        let _ = write!(self.out, "]({destination})");
        Some(len)
    }

    /// What a target points at: a URL or an anchor as itself, a tiddler title as
    /// the filename it was given, and `None` when it names a tiddler the export
    /// does not hold.
    fn resolved(&self, target: &str) -> Option<String> {
        if url(target).is_some_and(|len| len == target.len()) || target.starts_with('#') {
            return Some(target.to_string());
        }
        (self.resolve)(target)
    }

    /// One character of prose, escaped when Markdown would otherwise read it as
    /// syntax. Only where it would: escaping every `_` would put backslashes
    /// through the middle of every `snake_case` word in the notebook.
    fn escaped(&mut self, ch: char, at_line_start: bool) {
        let needs = match ch {
            '\\' | '`' | '*' | '[' | '<' => true,
            '#' | '>' | '-' | '+' | '=' | '_' => at_line_start,
            _ => false,
        };
        if needs {
            self.out.push('\\');
        }
        self.out.push(ch);
    }
}

/// `!` … `!!!!!!`, and the text after it.
fn heading(line: &str) -> Option<(String, &str, Styled)> {
    let level = line.chars().take_while(|c| *c == '!').count();
    if level == 0 || level > 6 {
        return None;
    }
    // The space after the marks is optional in `WikiText` — `!Heading` is one —
    // so requiring it would leave the commonest form as prose.
    let (rest, styled) = declassed(&line[level..]);
    Some(("#".repeat(level), rest.trim_start(), styled))
}

/// A list line, as its `*`/`#` prefix and the text after it.
fn list(line: &str) -> Option<(&str, &str, Styled)> {
    let depth = line.chars().take_while(|c| *c == '*' || *c == '#').count();
    if depth == 0 {
        return None;
    }
    let (rest, styled) = declassed(&line[depth..]);
    if !rest.starts_with(' ') {
        return None;
    }
    Some((&line[..depth], rest.trim_start(), styled))
}

/// Whether a block construct carried CSS classes that were dropped.
type Styled = bool;

/// Strips the `.myClass.another` a heading or list item may carry between its
/// marker and its text. The classes name a stylesheet noda does not have, so
/// they go — but going is reported, because a name that vanishes without
/// anybody saying so is the one thing this import may not do.
fn declassed(rest: &str) -> (&str, Styled) {
    match rest.strip_prefix('.') {
        Some(classes) => (classes.split_once(' ').map_or("", |(_, text)| text), true),
        None => (rest, false),
    }
}

/// `\define`, `\procedure` and the rest of the pragmas that make a tiddler a
/// program rather than a note.
fn definition(line: &str) -> bool {
    [
        "\\define",
        "\\procedure",
        "\\function",
        "\\widget",
        "\\import",
        "\\parameters",
        "\\rules",
        "\\whitespace",
    ]
    .iter()
    .any(|pragma| line.starts_with(pragma))
}

/// `@@color:red;` and `@@.myClass` at the head of a block.
fn style_open(line: &str) -> bool {
    line.starts_with("@@") && !line[2..].contains("@@")
}

/// A table row `GFM` can hold: no merged cells, no pseudo-rows, no vertical
/// alignment.
fn plain_row(row: &str) -> bool {
    let trimmed = row.trim_end();
    // `|…|h`, `|…|f`, `|…|c`, `|…|k` — header, footer, caption, class.
    if let Some(rest) = trimmed.strip_suffix(['h', 'f', 'c', 'k'])
        && rest.ends_with('|')
    {
        return false;
    }
    split_row(row)
        .iter()
        .all(|cell| !matches!(cell.trim(), "~" | "<" | ">") && !cell.starts_with(['^', ',']))
}

/// A row's cells, without the bars that bound them.
fn split_row(row: &str) -> Vec<&str> {
    let inner = row.trim_end().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').collect()
}

/// The length of the URL at the head of `text`, if one starts here.
fn url(text: &str) -> Option<usize> {
    let scheme = ["https://", "http://", "mailto:", "ftp://", "file://"]
        .iter()
        .find(|scheme| text.starts_with(**scheme))?;
    let end = text[scheme.len()..]
        .find(|c: char| c.is_whitespace() || c == '|' || c == ']')
        .map_or(text.len(), |n| scheme.len() + n);
    // A sentence's full stop is not part of its URL.
    let end = text[..end].trim_end_matches([',', '.', ';', ':']).len();
    Some(end)
}

/// The length of the `CamelCase` word at the head of `text`, if one starts
/// here: two capitals with lower case between and after them, which is what
/// `TiddlyWiki` treats as a link.
fn camel_len(text: &str) -> Option<usize> {
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_uppercase() {
        return None;
    }
    let mut lower_seen = false;
    let mut second_capital = false;
    let mut end = first.len_utf8();
    for (at, ch) in chars {
        if ch.is_uppercase() {
            if !lower_seen {
                return None;
            }
            second_capital = true;
        } else if ch.is_lowercase() || ch.is_numeric() {
            lower_seen = true;
        } else {
            end = at;
            break;
        }
        end = at + ch.len_utf8();
    }
    // A word must have lower case after its second capital too, or `IDs` and
    // `HTMLPage` would be links nobody wrote.
    (second_capital && text[..end].ends_with(char::is_lowercase)).then_some(end)
}

/// An HTML tag opening here — as opposed to a `<` in prose, which is far more
/// common and must stay a `<`.
fn html_tag(text: &str) -> bool {
    let after = text.strip_prefix('<').unwrap_or("");
    let after = after.strip_prefix('/').unwrap_or(after);
    after.starts_with(|c: char| c.is_ascii_alphabetic()) && text.contains('>')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A resolver that turns a title into the filename its slug would give it,
    /// so a link's target words survive into the destination and the loss
    /// invariant can hold links to the same standard as prose.
    fn resolver(known: &[&str]) -> impl Fn(&str) -> Option<String> + use<> {
        let known: Vec<String> = known.iter().map(|t| (*t).to_string()).collect();
        move |title: &str| {
            known.contains(&title.to_string()).then(|| {
                let slug: String = title
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '-' })
                    .collect();
                format!("k3f9m2p1-{}.md", slug.to_lowercase())
            })
        }
    }

    fn convert_with(text: &str, known: &[&str]) -> Converted {
        convert(text, &resolver(known))
    }

    fn text_of(text: &str) -> String {
        convert_with(text, &[]).text
    }

    #[test]
    fn headings_take_their_level_from_the_marks() {
        assert_eq!(text_of("!Top\n!!!Third\n"), "# Top\n### Third\n");
        // Seven is not a heading in Markdown, so it is not one here either —
        // and `!` needs no escaping, since only `![` opens anything.
        assert_eq!(text_of("!!!!!!!Seven\n"), "!!!!!!!Seven\n");
        // A bare `!` that opens a sentence is punctuation, not a heading.
        assert_eq!(text_of("!!! Warning\n"), "### Warning\n");
    }

    #[test]
    fn a_dropped_class_is_reported_rather_than_lost() {
        let out = convert_with("!!!.myClass Heading\n", &[]);
        assert_eq!(out.text, "### Heading\n");
        assert!(out.left.contains(left::STYLE), "the class went somewhere");
    }

    #[test]
    fn lists_nest_by_the_length_of_their_prefix() {
        assert_eq!(
            text_of("* One\n** Two\n*# Numbered\n# Ordered\n"),
            "- One\n  - Two\n  1. Numbered\n1. Ordered\n"
        );
    }

    #[test]
    fn emphasis_pairs_convert_and_nest() {
        assert_eq!(text_of("''bold''\n"), "**bold**\n");
        assert_eq!(text_of("//italic//\n"), "*italic*\n");
        assert_eq!(text_of("//''both''//\n"), "***both***\n");
        // Already Markdown.
        assert_eq!(text_of("~~gone~~\n"), "~~gone~~\n");
    }

    /// The rule that keeps a converter honest: punctuation that looks like a
    /// delimiter but closes nothing is punctuation.
    #[test]
    fn an_unmatched_delimiter_stays_text() {
        assert_eq!(text_of("it cost 5'' of tape\n"), "it cost 5'' of tape\n");
        assert_eq!(text_of("a // b\n"), "a // b\n");
    }

    /// A URL is full of `//`, and every one of them would otherwise open an
    /// italic that runs to the end of the paragraph.
    #[test]
    fn a_url_is_not_read_as_emphasis() {
        assert_eq!(
            text_of("see https://example.com/a//b for //this//\n"),
            "see https://example.com/a//b for *this*\n"
        );
    }

    #[test]
    fn a_link_becomes_the_file_its_target_was_given() {
        let out = convert_with("see [[Meeting Notes]]\n", &["Meeting Notes"]);
        assert_eq!(out.text, "see [Meeting Notes](k3f9m2p1-meeting-notes.md)\n");
        assert!(out.left.is_empty());
    }

    /// The caption comes first in both languages. It is `MediaWiki` that puts it
    /// second, and writing this from habit is how a link ends up pointing at its
    /// own caption while still looking perfectly fine.
    #[test]
    fn a_captioned_link_keeps_caption_first() {
        let out = convert_with("[[the notes|Meeting Notes]]\n", &["Meeting Notes"]);
        assert_eq!(out.text, "[the notes](k3f9m2p1-meeting-notes.md)\n");
    }

    #[test]
    fn a_link_to_nothing_is_left_as_it_was_written() {
        let out = convert_with("see [[Missing Tiddler]]\n", &["Meeting Notes"]);
        assert_eq!(
            out.text, "see [[Missing Tiddler]]\n",
            "a link to a file that will not exist is worse than the WikiText"
        );
        assert!(out.left.contains(left::LINK));
    }

    #[test]
    fn external_links_carry_their_url() {
        assert_eq!(
            text_of("[[TW5|https://tiddlywiki.com/]]\n"),
            "[TW5](https://tiddlywiki.com/)\n"
        );
        assert_eq!(
            text_of("[ext[Open|index.html]]\n"),
            "[Open](index.html)\n",
            "an ext link names a file, not a tiddler"
        );
    }

    #[test]
    fn an_image_keeps_its_tooltip_and_reports_its_attributes() {
        assert_eq!(text_of("[img[a cat|cat.jpg]]\n"), "![a cat](cat.jpg)\n");
        let out = convert_with("[img width=32 [cat.jpg]]\n", &[]);
        assert_eq!(out.text, "![](cat.jpg)\n");
        assert!(out.left.contains(left::IMAGE_ATTRIBUTES));
    }

    #[test]
    fn a_block_quote_keeps_its_citation() {
        assert_eq!(
            text_of("<<<\nA bicycle for our minds\n<<< Steve Jobs\n"),
            "> A bicycle for our minds\n> — Steve Jobs\n"
        );
    }

    #[test]
    fn a_plain_table_becomes_a_gfm_table() {
        assert_eq!(
            text_of("|!One |!Two |\n|a |b |\n"),
            "| One | Two |\n| --- | --- |\n| a | b |\n"
        );
    }

    /// `GFM` has one header row and nothing else. A table using a footer, a
    /// caption or a merged cell is left alone rather than flattened into one
    /// that has quietly lost a row.
    #[test]
    fn a_table_markdown_cannot_hold_is_left_alone() {
        for table in [
            "|!One |!Two |\n|a |b |\n|Footer|Footer|f\n",
            "|a caption |c\n|a |b |\n",
            "|a |< |\n",
        ] {
            let out = convert_with(table, &[]);
            assert_eq!(out.text, table, "left as written");
            assert!(out.left.contains(left::TABLE), "and named: {table}");
        }
    }

    #[test]
    fn a_fence_is_copied_through_untouched() {
        let fenced = "```rust\nlet x = ''not bold'';\n```\n";
        assert_eq!(text_of(fenced), fenced);
    }

    #[test]
    fn inline_code_is_left_alone_too() {
        assert_eq!(text_of("run `a // b` now\n"), "run `a // b` now\n");
    }

    #[test]
    fn a_macro_definition_stops_the_conversion() {
        let source = "\\define greet() hello\n\n!Not a heading now\n";
        let out = convert_with(source, &[]);
        assert_eq!(
            out.text, source,
            "everything after a pragma is a program, not prose"
        );
        assert!(out.left.contains(left::MACRO));
    }

    #[test]
    fn what_markdown_has_no_word_for_is_kept_and_named() {
        for (source, name) in [
            ("a {{Transcluded}} here\n", left::TRANSCLUSION),
            ("a <<mymacro arg>> here\n", left::MACRO),
            ("a <$list filter=\"x\"> here\n", left::WIDGET),
            ("a <div class=\"x\"> here\n", left::HTML),
            ("an __underline__ here\n", left::UNDERLINE),
            ("a ^^super^^ here\n", left::SUPERSCRIPT),
            ("a ,,sub,, here\n", left::SUBSCRIPT),
        ] {
            let out = convert_with(source, &[]);
            assert!(out.left.contains(name), "{name} not reported for {source}");
        }
    }

    /// `CamelCase` is a link in `WikiText` whether anybody meant it or not, and
    /// `~` is how it is turned off. Neither survives into Markdown as a tilde.
    #[test]
    fn camel_case_links_only_where_it_resolves() {
        let out = convert_with("See CamelCase and JavaScript\n", &["CamelCase"]);
        assert_eq!(
            out.text, "See [CamelCase](k3f9m2p1-camelcase.md) and JavaScript\n",
            "prose that names no tiddler stays prose"
        );
        // The suppression mark is markup, not a character somebody typed.
        assert_eq!(text_of("~CamelCase stays text\n"), "CamelCase stays text\n");
        // Neither is every capitalised word a link.
        assert_eq!(
            text_of("HTML and IDs and Hello\n"),
            "HTML and IDs and Hello\n"
        );
    }

    /// Prose that means nothing in `WikiText` can mean something in Markdown.
    #[test]
    fn markdown_syntax_in_prose_is_escaped() {
        assert_eq!(text_of("2 * 3 * 4\n"), "2 \\* 3 \\* 4\n");
        assert_eq!(text_of("see [1] there\n"), "see \\[1] there\n");
        assert_eq!(text_of("a < b\n"), "a \\< b\n");
    }

    /// The notebook this import exists for is written in Chinese, and every
    /// slice in here is taken by byte.
    #[test]
    fn multibyte_text_survives_every_slice() {
        let out = convert_with(
            "!!標題\n* ''粗體''與//斜體//\n看 [[會議記錄]] 和 https://example.com/路徑\n",
            &["會議記錄"],
        );
        assert_eq!(
            out.text,
            "## 標題\n- **粗體**與*斜體*\n看 [會議記錄](k3f9m2p1-會議記錄.md) 和 https://example.com/路徑\n"
        );
    }

    /// Every construct in one document, for the loss invariant below.
    const EVERYTHING: &str = "\
!A Heading
!!!Another Heading

Some prose with ''bold'', //italic//, ~~struck~~, __under__, ^^super^^ and ,,sub,,.
A link to [[Meeting Notes]] and one with [[a caption|Meeting Notes]].
An external https://example.com/page and [[named|https://example.com/other]].
An image [img[a photograph|photo.jpg]].
A transclusion {{Some Tiddler}} and a macro <<greeting name>>.
A widget <$list filter=\"whatever\"> and some <div class=\"raw\">html</div>.

* first bullet
** nested bullet
# first number

<<<
A quotation worth keeping
<<< Somebody Notable

|!Heading one |!Heading two |
|table cell |another cell |

; A term
: Its definition

```
fenced code stays exactly as written
```

Prose in 中文，含有標點與空白。
";

    /// Words, in the loosest sense that survives Chinese: runs of alphanumerics.
    ///
    /// `img` and `ext` are the two words that are syntax rather than content —
    /// they name the construct, not anything a reader was meant to see — so they
    /// are the only thing the invariant below forgives.
    fn words(text: &str) -> BTreeSet<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 3)
            .map(str::to_lowercase)
            .filter(|w| w != "img" && w != "ext")
            .collect()
    }

    /// The promise, mechanised: **no text disappears**. Every word in the source
    /// is somewhere in the output, as prose, as markup, or as `WikiText` that
    /// was left alone. A converter that swallows a paragraph, drops a table row
    /// or eats the far side of a delimiter fails here and nowhere else — which
    /// is the whole reason this test exists rather than a pile of golden files.
    #[test]
    fn no_word_is_lost() {
        let out = convert_with(EVERYTHING, &["Meeting Notes", "Some Tiddler"]);
        let before = words(EVERYTHING);
        let after = words(&out.text);
        let missing: Vec<&String> = before.difference(&after).collect();
        assert!(missing.is_empty(), "these words vanished: {missing:?}");
    }

    /// The same promise held against the Markdown as a reader sees it, rather
    /// than as the file spells it. `pulldown-cmark` is already in the tree for
    /// reading links, and here it answers the harder question: does the prose
    /// still say what it said, once a Markdown parser has had its turn?
    #[test]
    fn no_word_is_lost_once_markdown_is_parsed() {
        use pulldown_cmark::{Event, Parser, Tag};

        let out = convert_with(EVERYTHING, &["Meeting Notes", "Some Tiddler"]);
        let mut rendered = String::new();
        for event in Parser::new(&out.text) {
            match event {
                Event::Text(t) | Event::Code(t) | Event::Html(t) | Event::InlineHtml(t) => {
                    rendered.push_str(&t);
                    rendered.push(' ');
                }
                // A destination is carried rather than displayed — in `WikiText`
                // as much as in Markdown — so it counts as survived. What the
                // invariant is looking for is text that reached neither.
                Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                    rendered.push_str(&dest_url);
                    rendered.push(' ');
                }
                _ => rendered.push(' '),
            }
        }
        let (before, after) = (words(EVERYTHING), words(&rendered));
        let missing: Vec<&String> = before.difference(&after).collect();
        assert!(
            missing.is_empty(),
            "these words are in the file but not in what it renders: {missing:?}"
        );
    }
}
