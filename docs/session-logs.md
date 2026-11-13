# Session logs

SSHCraft can record what a terminal shows: SSH sessions, the local terminal and serial
connections. Recordings are kept sealed on the device, can carry bookmarks and comments, can be
searched, exported and deleted, are removed after a time you choose and can sync with your own
sync server.

## What is recorded

Only the **output**: what the server, the shell or the serial device sends and the terminal
shows. Keys you type are not recorded. A password typed without echo (sudo, ssh, a passphrase
prompt) therefore never lands in a log; a password that a program prints on the screen does.

One log covers the life of one tab or pane, reconnects included. It ends when the session ends
or the tab is closed. A log stops recording at **64 MiB** and is then marked as cut.

## Switching it on

- *Settings → Session logs → Record sessions* is the default for every session (off at first).
- A host or group can decide for itself: *Record sessions* in its form is *From the group*,
  *Yes* or *No*. A host follows its groups up to the top; where none decides, the setting above
  applies. Local and serial sessions follow the setting.
- Without the unlocked vault nothing is recorded.

While a pane records, a small **Rec** mark sits in its lower right corner.

## Bookmarks and comments

- **Ctrl+Shift+L** in a terminal sets a bookmark at the output so far, named with the time.
- In the viewer, *Bookmark* and *Comment* add one at the top visible line. A bookmark has an
  optional name (up to 100 characters), a comment 1 to 2000 characters over several lines.
  At most 500 per log.
- Click one to jump there; the bin removes it.

## The logs page

*Logs* in the sidebar lists every log with its host, start, duration, size and place (*This
device only* or the sync server). Filter by host, or by *Local and serial*.

- **Search** (from two characters) looks through all logs, case-insensitive, in the text as a
  reader sees it (without colors and cursor movement). A match opens the log and finds the
  text in it. At most 200 matches.
- **Open** a log to play it back in a read-only terminal, with search, bookmarks and comments.
- **Export** as *text* (`.txt`, without escape sequences) or *raw* (`.log`, the bytes as
  recorded; `cat` on a terminal plays them back).
- **Delete** selected logs, or all that the filter shows. A deleted host leaves its logs; they
  show *Deleted host*.

## Keeping

*Settings → Session logs → Keep*: until you delete them, or 7, 30, 90 or 365 days. Older finished
logs are deleted when the app starts and then every hour. A log that was still recording when
the app stopped is ended at the next start, at the time of its last stored part.

## Sync

Per server, *Settings → Sync → Sync session logs* (off at first). With it on:

- A log lives where its host lives. It goes to the server once it has ended, part by part (32
  KiB each), the header with title, times, bookmarks and comments last.
- Logs go into your **personal area** only, also those of a host in a share: recipients of a
  share never get your logs. A log record in a share is ignored.
- Moving a host moves its logs; removing a server keeps them on this device. Logs of local and
  serial sessions stay on the device.
- Deleting a log, by hand or because it expired, deletes it on your other devices too.
- Turning it on sends the logs already there and fetches everything from the start again.
  With it off, log records are skipped without a warning.

The server sees how many log records there are and how big they are, never their content.
Versions of SSHCraft without session logs cannot read log records: update every device before
turning log sync on.

## Storage

Logs are in the local database: each part and each header sealed with the vault, bound to its
log and position, so a part copied to another place does not open (the viewer then reports the
log as damaged). In plain text there are only ids, the host id, start and end times, sizes and
the place.
