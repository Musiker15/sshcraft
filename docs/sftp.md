# SFTP

The **SFTP** button of a host opens its files in a tab of their own. On the left you see this
computer, on the right the host. Below them are the transfers. If the window is narrower than
700 points, only one side shows at a time and two chips switch between them. On Android only the
host's side is there, because the app cannot browse the device's folders. There **Upload** picks a
file through the system's picker and sends it into the shown folder under its own name, beside
anything of that name, and **Download** (one file selected) asks the picker where it goes. An open
SFTP tab keeps the app running in the background like a terminal does, so the connection holds
while the picker is open.

## Opening

SFTP signs in the same way a terminal does: the same host key check, sign-in and jump hosts. If
*Reuse connections* is on and a terminal to the host is open, SFTP rides on that connection and
does not ask again. A server that offers no SFTP says so at once. Closing the tab ends the SFTP
session, cancels its transfers, and closes the connection when nothing else uses it.

## Browsing

- Each side has the folder above and *Read again* buttons, and shows its path.
- The filter keeps only names that contain what you type.
- *Hidden files* also shows names that start with a dot.
- The columns are name, size and modification date, plus permissions on the host's side when the
  pane is wide enough. A click on a column heading sorts by it, a second click reverses the
  order. Folders always come first.
- A click selects an entry. Ctrl+click (Cmd+click on macOS) adds or removes one.
- A double click opens a folder or a link. On a file it starts a transfer to the other side.

## Actions

On the host's side:

- **New folder**: a name in the shown folder.
- **Rename**: only within the same folder. A name that is taken is refused.
- **Permissions**: octal, at most `7777`.
- **Delete**: first counts what would go. The dialog names the number of files and folders and
  deletes only after you confirm.

**Upload** (on this computer's side) and **Download** (on the host's side) send the selection
into the folder the other side shows. Dragging entries from one side onto the other does the
same.

## Transfers

Files go one after another. Each file is read or written with 16 requests at once, and at most two
jobs run at the same time while the rest wait. The strip under the sides shows each job with its
progress. A running job can be cancelled, and *Clear finished* removes the jobs that ended.

A file is first written as `.<name>.sshcraft-part` and renamed only when it is complete. If you
cancel, or a file fails for any other reason, its part is deleted, so nothing half-written stays
behind, on the server or here.

The one exception is a lost connection. Then the part stays, the host's side shows *Reconnect*,
and the job gets *Try again*. Trying again leaves the files that already got there and continues
the interrupted one from where its part ends. SSHCraft continues only a part it left itself
during this run, and only one that is not longer than the file. Any other part, such as one from
an earlier start of the app, is replaced, never written onto. A cancelled job, or one that ended
with some files failed, can be tried again the same way: the files that made it are not sent
twice.

When a name is already in the target folder, the pane asks before it starts:

- **Skip** leaves those files alone.
- **Keep both** gives the new one a free name, `name (2).ext`.
- **Replace** overwrites.

A program that calls the commands passes the rule itself: `skip`, `rename`, `overwrite`, or
`newer`, which replaces only an older file (what a sync uses). Without one, the files are skipped.

## Copying from server to server

With the files of two hosts open, each in its own tab or pane, select entries on one host's side
and choose **Copy to…**. The dialog lists the other open hosts with the folder each one shows,
and the copy goes into that folder. This works on Android too.

The data goes through this computer: SSHCraft reads from one server and writes to the other.
Nothing runs on either server, and neither server needs to reach the other. A copy follows the
same rules as an upload: part files, the question when a name is taken, *Try again* after a lost
connection. Names from the first server go to the second only if they are plain names. The job
shows in the transfers of both panes. Closing either pane stops it, and its part is deleted.

## Syncing two folders

**Sync folders…** on this computer's side brings the folder shown here and the folder shown on
the host up to date, one way at a time:

- **Upload newer files** sends what is in the folder here into the folder on the host.
- **Download newer files** brings what is in the folder on the host into the folder here.

The contents go into the other folder, including subfolders, not into a new folder of the same
name. A file is sent only when it is missing at the target or older there. A file that changed
at the target since the last sync stays as it is. Nothing is deleted on either side, so a file
removed here stays on the host. A file that is sent takes its source's modification time, so the
next sync sees the two as the same. The job counts files that were already the same as skipped.

A sync follows the rules of a transfer: part files, *Try again* after a lost connection, names
from the server checked before they become a local path. The folder on the host must exist; the
sync makes none. Without a local side (Android) there is no sync. A program passes the rule
`newer` itself to `sftp.upload`, `sftp.download` or `sftp.copy` to get the same comparison.

## Editing a file in a text editor

**Open in editor**, with one file on the host selected, opens it in the editor you named under
*Settings → Terminal → Text editor*. Nothing else ever opens a file: not the program the system links
to its type, since opening a downloaded file there can run it.

- The editor is a program file you choose. It starts with the file as its only argument, never
  through a shell.
- The file is copied into SSHCraft's data folder first. Files over 16 MiB and anything that is
  not a plain file are refused.
- Every time you save, the copy goes back to the host: under a part name first, renamed over the
  file when it is complete, with the file's permissions kept.
- If the file changed on the host since you opened it, nothing is sent. The SFTP tab asks
  whether to overwrite the host's version with yours; *Cancel* keeps the host's, and your next
  save asks again.
- *Stop editing* in the SFTP tab, closing the tab or quitting SSHCraft deletes the copy. A copy
  left behind by a crash goes at the next start.
- Without a local side (Android) there is no editing.

## What is refused, and why

SSHCraft treats everything the server sends as hostile:

- A name from the server becomes part of a local path only if it is a plain name here.
  That rules out `.` and `..`, any `/` or `\`, NUL and control characters, and names longer than
  255 bytes. On Windows it also rules out the reserved names (`CON`, `NUL`, `COM1` and so on),
  `:`, the characters Windows does not allow, and a dot or space at the end. Such an entry is left
  out with a note in the job, and the rest of the folder still comes. A download never writes
  outside the folder you chose.
- Paths on the host are made canonical as text, never above `/`, and may be at most
  4096 bytes long. `/` itself is never deleted.
- A link to a folder is not followed when copying or deleting, so there are no loops
  and nothing outside gets deleted. A link to a file is copied as the file.
- A listing stops after 100,000 entries and says so. A copy or delete goes at most 64
  folders deep and touches at most a million entries. A folder with more entries than a listing
  gives is refused for copying or deleting, because half a delete would be worse than none.
- Names and messages from the server are shown with their control characters removed,
  so an escape sequence in one has no effect.

SFTP speaks only the protocol, on its own channel. No name ever goes into a command line.

## For programs

The commands are `sftp.open`, `sftp.close`, `sftp.list`, `sftp.local_list`, `sftp.mkdir`,
`sftp.rename`, `sftp.chmod`, `sftp.count`, `sftp.delete`, `sftp.upload`, `sftp.download`,
`sftp.copy`, `sftp.sync`, `sftp.download_file`, `sftp.transfers`, `sftp.cancel`, `sftp.clear_transfers`,
`sftp.retry`, `sftp.reconnect`, `sftp.edit`, `sftp.edits`, `sftp.edit_save`, `sftp.edit_sync` and
`sftp.edit_stop`. Their parameters are in the palette's
command list and in `crates/engine/src/cmd/sftp.rs`. Progress comes as the engine event
`TransferStatus`. Agents have no access to any of them.

## Not yet

Some things are not there yet ([gaps.md](gaps.md)):

- opening a file in the program the system links to its type, since that can run it; only the
  text editor you name opens files;
- a sync in both directions at once, or one that deletes what is gone from the source.
