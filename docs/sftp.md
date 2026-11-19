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

A program that calls the commands passes the rule itself. Without one, the files are skipped.

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
`sftp.transfers`, `sftp.cancel`, `sftp.clear_transfers`, `sftp.retry` and `sftp.reconnect`. Their parameters are in the palette's
command list and in `crates/engine/src/cmd/sftp.rs`. Progress comes as the engine event
`TransferStatus`. Agents have no access to any of them.

## Not yet

Some things are not there yet ([gaps.md](gaps.md)):

- opening or editing a file in another program, since opening a downloaded file can run it;
- syncing a folder;
- copying from server to server;
