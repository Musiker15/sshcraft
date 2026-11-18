# Android

SSHCraft runs on Android tablets and phones with Android 13 or newer (arm64, armv7 or x86_64). It is the same app as on the
desktop, in a form made for a touch screen. There is no Play Store listing: the APK is attached to
the GitHub releases and installed by hand.

## Install

1. Download the APK for your device from the
   [releases](https://github.com/Musiker15/sshcraft/releases): `sshcraft-<version>-android-arm64.apk`
   for nearly every phone and tablet, `-armv7.apk` for an older device with a 32-bit processor,
   `-x86_64.apk` for a Chromebook or an x86 tablet.
2. Open it. Android asks once whether the browser or file manager may install apps; allow it
   for that app.
3. Start SSHCraft from the launcher.

An update is installed the same way, over the installed app; hosts, keys and settings stay. The
APK is signed with SSHCraft's release key. Its certificate's SHA-256 fingerprint is

```
7A:FA:4C:38:E6:95:00:99:BB:08:27:26:21:AE:C9:0B:E2:0A:F8:80:9E:2F:EB:8E:0F:6B:53:81:22:EB:F9:C8
```

`apksigner verify --print-certs sshcraft-<version>-android-arm64.apk` shows it. Android refuses
an update signed with another key; an APK with a different fingerprint is not from SSHCraft.

## Getting your hosts onto the tablet

Under **Settings > Sync**, add your sync server and choose **Join with account key**, then paste
the account key (from your password manager): with Ctrl+V on a hardware keyboard, or with the
**Paste** button that shows in the lower right corner while a text field has the keyboard. Hosts and groups
arrive as on any other device, keys and passwords too when *Keys and passwords* is on
([sync.md](sync.md)). Snippets stay on each device. Hosts can also be added by hand, and a key
can be imported by pasting it. Android has no SSH agent: for a host that uses a key from Proton
Pass on the PC, import that key on the tablet and leave the host set to the agent.

## Typing

Tap the terminal to open the on-screen keyboard. The app moves up so the line you type stays
visible. What the keyboard pastes from its own clipboard, swiped words and umlauts arrive as
typed; text of more than one character goes to the terminal as one paste. Under the terminal is a bar of the keys an on-screen keyboard lacks:

| Key | What it does |
|---|---|
| Esc, Tab | as on a keyboard |
| Ctrl, Alt | held for the next key or character, typed on the bar or the keyboard; tap again to let go |
| arrows, Home, End, PgUp, PgDn | as on a keyboard, also in full-screen programs |
| `\|` `~` `/` `-` `_` | characters on-screen keyboards hide |
| Paste | the text on Android's clipboard |
| Copy | shown while text is selected |

With a hardware keyboard everything works as on the desktop. Ctrl+V pastes in fields, and
Ctrl+Shift+V or Shift+Insert paste in the terminal, both from Android's clipboard.

Drag a finger over the terminal to scroll back. To select, press and hold, then drag; **Copy** on
the bar puts the selection on Android's clipboard.

## What is different from the desktop

- On a narrow screen (a phone, or a tablet held upright) the pages sit under a top bar, and its
  menu button opens the list of pages. Wider screens show a rail of symbols on the left, and from
  1000 points the sidebar with the page names.
- The vault's master key is kept in Android's credential store, encrypted with a key in the
  Android Keystore that never leaves the device. The app's data is excluded from Android's
  backup and from the transfer to a new device, so a new device starts empty and gets its data
  through sync.
- No local terminal or serial port, and no file dialogs: importing hosts from a file, exporting,
  backups to a file, saving a session log and `known_hosts` files are desktop only.
- No AI agents: the control channel, MCP and `sshcraft-cli` are desktop only.
- No dropdown window, no desktop shortcuts, no X11 forwarding, no FIDO keys and no screen reader
  support yet.
- A word the keyboard is still composing shows where it will go, in the terminal and in a field,
  and the keyboard's choice replaces it.
- The sync server's certificate is checked against Mozilla's list of public authorities, not
  Android's own store, so a server with a certificate from a private or user-installed authority
  is not trusted on Android.
- While sessions are open, a notification ("2 sessions open") keeps SSHCraft and its network
  running in the background; Android stops a backgrounded app's network otherwise. It goes away
  with the last session. Android asks once whether SSHCraft may show notifications.
- The app's messages (a long command ended, a message from a server) come as notifications,
  while SSHCraft is on screen or just went to the background.

The open points are in [gaps.md](gaps.md).

## Building it

The APK is built on a desktop with the Android SDK, the NDK and a JDK (javac and d8 build the
small Java part in `apps/android/java`); the shared CI has no
Android SDK, so this runs locally.

1. Install the SDK command-line tools, then `platform-tools`, `platforms;android-36`,
   `build-tools;37.0.0` and an NDK (30 or newer) with `sdkmanager`.
2. Set `ANDROID_HOME` to the SDK (and `NDK_HOME` if the NDK is elsewhere), and have `java`
   on the `PATH` or `JAVA_HOME` set.
3. `rustup target add aarch64-linux-android`.

```sh
cargo xtask android --check      # clippy for Android on the crates that differ there
cargo xtask android --debug      # a debug APK, signed with a debug key
cargo xtask android              # the release APK in dist/android/
cargo xtask android --install    # and install it with adb on the connected device
```

The release key is not in the repository. `SSHCRAFT_ANDROID_KEYSTORE` names the keystore (key
alias `sshcraft`) and `SSHCRAFT_ANDROID_KEYSTORE_PASS_FILE` a file with its password; without them
the APK gets a debug key, which can neither update a release nor be updated by one.
