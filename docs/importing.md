# Importing

*Bringing a notebook in from somewhere else. Part of the [noda README](../README.md).*


| Command | Description |
| --- | --- |
| `noda import tiddlywiki <file>... [--no-convert]` | Import a TiddlyWiki 5 export: the JSON `export all` writes, or a saved single-file wiki. |

The format is named rather than sniffed. Guessing wrong would import somebody's notes as the
wrong thing, quietly, which is the one failure an import must not have.

```
$ noda import tiddlywiki notes.json
imported  1693 notes from tiddlywiki
converted 1678 notes

left as WikiText, and named in each note's `unconverted:` field:
  915 notes macro
  239 notes transclusion
  29 notes table

not imported:
  337 system tiddler
  12 not text (image/webp)
```

## A wiki exported in pieces

Several files are one import rather than several, because a wiki taken in pieces has links
running between the pieces:

```
$ noda import tiddlywiki part1.json part2.json part3.json
```

Every file is read before anything is written, so one that will not parse stops the import
before it has touched the notebook and says which file it was. Exports taken in pieces
overlap, and a note given twice arrives once — the first copy lands, the second is reported.

Bringing a wiki in over several sittings works too: the link rewriting starts from what the
notebook already holds, so a note imported today can link to one that arrived last week. What
cannot resolve is a link to a tiddler no import has brought in yet, and that is left as
WikiText and named, like everything else that could not be finished.

## Two commits, so nothing can be lost

An import writes **two** commits: the first holds every note exactly as the wiki wrote it,
the second holds the conversion.

```
$ noda log -n 2
  7d1016e  2026-08-02 22:50  import: convert 1678 notes from tiddlywiki
  bb81bb7  2026-08-02 22:50  import: 1693 notes from tiddlywiki
```

So `noda diff` shows you the whole conversion before it goes anywhere, and
`noda restore <note> HEAD~1` brings any note back to the text the export actually contained.
The original is not copied into the frontmatter, because git already keeps it and keeps it
better — the same reasoning behind every other command here being a commit.

## What converts, and what does not

`''bold''`, `//italic//`, `!` headings, `*`/`#` lists, `<<<` quotes, `[[links]]`, `[img[…]]`
and fenced code all have a Markdown form, so they get one. A link's target is a tiddler
*title* and noda's is a *filename*, which is why the rewrite is the second pass: the ids do
not exist until the notes do.

Anything Markdown has no word for — a transclusion, a macro, a widget, a table with a footer
or a merged cell — is **copied through as WikiText, exactly as it was written**, and named in
the note's own frontmatter:

```
---
title: Some note
source_key: Some Note
unconverted: macro, table
---
```

Unconverted WikiText is findable and fixable. Markdown that looks right and says something
else is neither, so nothing here is guessed.

`noda doctor` is the handle on that field, and needs no flag for it — the frontmatter is
already parsed:

```
$ noda doctor
3 notes carry text an importer did not convert
  3 notes macro
  1 note table
  for example:
    k3f9m2p1-some-note.md
```

It is a frontmatter field rather than a tag because tags belong to whoever writes the notes;
filing noda's paperwork among them would be noda using your drawer. Delete the field once a
note is dealt with and the count goes down.

`--no-convert` writes the first commit and stops, leaving the WikiText for you.

## Times, tags and fields

TiddlyWiki's `created` and `modified` are `YYYYMMDDhhmmssXXX` in UTC; they become RFC 3339
with their milliseconds intact, and noda never restates them again. A `tags` field is a title
list, so `[[26.04 Occam's razor]]` arrives as one tag with its spaces. Every other field the
wiki had — `creator`, `modifier`, whatever you invented — is carried into the frontmatter
untouched, and `source_key` records what the wiki called the note, which is what makes a
second import say "already imported" instead of making a second copy.

What is not a note is reported rather than dropped: system tiddlers under `$:/`, pictures and
other binaries, empty tiddlers, and anything carrying a title or a tag noda's own files
cannot spell.

