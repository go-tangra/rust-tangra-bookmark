---
title: Back up and restore
summary: Export your organisation's bookmarks and shares to a file and import them again, keeping or replacing what exists.
kind: guide
order: 30
tags: [bookmarks, backup, restore]
requires: backup:manage
---
## Export

Open **Bookmark backup** and choose **Export**. The file holds every
bookmark and every share of your organisation, with their IDs. It is a JSON
document with `schema_version: 1`. Nothing of other organisations is
included.

Treat the file as confidential: it contains every address and who may see
it.

## Import

Choose **Import**, pick a backup file and a mode:

- **Skip** (default): bookmarks and shares that already exist stay as they
  are; only missing ones are added. A bookmark is "the same" when it has the
  same ID; a share when it gives the same level to the same person, role or
  everyone on the same bookmark.
- **Overwrite**: what the file holds replaces what exists.

The import is all or nothing. When one entry is invalid (for example an
address that is not `http` or `https`), nothing is imported and the problem
is named, such as `bookmarks[3].url`. A share of a bookmark that is neither
in the file nor in your organisation is skipped.

The result shows how many bookmarks and shares were imported and skipped.
Importing a backup taken in another organisation writes into yours only.
