# Importing

*Bringing a notebook in from somewhere else. Part of the [noda README](../README.md).*


| Command | Description |
| --- | --- |
| `noda import tiddlywiki <file>... [--no-convert]` | Import a TiddlyWiki 5 export: the JSON `export all` writes, or a saved single-file wiki. |

The format is named rather than sniffed, because a wrong guess would quietly import notes as the
wrong thing.

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

`noda diff` shows the conversion; the commit before it holds the originals
```

A note the filesystem will not take — a name it refuses, a full disk — is one more line under
`not imported:` rather than the end of the run. Where the original lands but its conversion cannot
be written, the note keeps the wiki's own text and is listed under
`imported, but left as the source wrote them:`.

## A wiki exported in pieces

Several files are one import, because links run between the pieces:

```
$ noda import tiddlywiki part1.json part2.json part3.json
```

Every file is read before anything is written, so one that will not parse stops the import before
it touches the notebook, naming the file. A note given twice arrives once — the first copy lands,
the second is reported.

Importing over several sittings works too: link rewriting starts from what the notebook already
holds. A link to a tiddler no import has brought in yet is left as WikiText and named.

## Two commits, so nothing can be lost

An import writes **two** commits: the first holds every note exactly as the wiki wrote it, the
second the conversion.

```
$ noda log -n 2
  7d1016e  2026-08-02 22:50  import: convert 1678 notes from tiddlywiki
  bb81bb7  2026-08-02 22:50  import: 1693 notes from tiddlywiki
```

So `noda diff` shows the whole conversion, and `noda restore <note> HEAD~1` brings any note back
to the text the export contained. The original is not copied into the frontmatter; git already
keeps it.

## What converts, and what does not

`''bold''`, `//italic//`, `!` headings, `*`/`#` lists, `<<<` quotes, `[[links]]`, `[img[…]]` and
fenced code all have a Markdown form. A link's target is a tiddler *title* and noda's is a
*filename*, which is why links are rewritten in a second pass: the ids do not exist until the notes
do.

Anything Markdown has no word for — a transclusion, a macro, a widget, a table with a footer or a
merged cell — is **copied through as WikiText, exactly as written**, and named in the note's
frontmatter:

```
---
title: Some note
source_key: Some Note
unconverted: macro, table
---
```

Unconverted WikiText is findable and fixable; Markdown that looks right and says something else is
neither, so nothing is guessed.

`noda doctor` reports that field with no flag needed:

```
$ noda doctor
3 notes carry text an importer did not convert
  3 notes macro
  1 note table
  for example:
    k3f9m2p1-some-note.md
```

It is a frontmatter field rather than a tag because tags belong to whoever writes the notes. Delete
the field once a note is dealt with and the count goes down.

`--no-convert` writes the first commit and stops, leaving the WikiText for you.

## Times, tags and fields

TiddlyWiki's `created` and `modified` are `YYYYMMDDhhmmssXXX` in UTC; they become RFC 3339 with
their milliseconds intact. A `tags` field is a title list, so `[[26.04 Occam's razor]]` arrives as
one tag with its spaces. Every other field — `creator`, `modifier`, anything you invented — is
carried into the frontmatter untouched, and `source_key` records what the wiki called the note, so
a second import says "already imported" instead of making a second copy.

What is not a note is reported rather than dropped: system tiddlers under `$:/`, pictures and other
binaries, empty tiddlers, and anything with a title or tag noda's files cannot spell.
