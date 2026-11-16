# Asset attribution

Every non-code asset in this repository (images, icons, sounds, example files) is listed here with
its author, source and licence. `cargo xtask assets` (part of `cargo xtask ci`) fails if an asset
file is missing from this table.

**Policy:** SSHCraft contains **no Termius (or any other vendor's) icons, images, artwork, fonts,
themes or sample files**. Every asset is original work by SSHCraft contributors or third-party
material under a licence from the list in [AGENTS.md](AGENTS.md). Font files are not added here:
shared fonts live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), an
optional build input (`CRAFT_FONTS_DIR`).

| Asset | Author | Source | Licence | Notes |
|---|---|---|---|---|
| `assets/app-icon/hicolor/128x128/apps/de.musiker15.sshcraft.png` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/hicolor/16x16/apps/de.musiker15.sshcraft.png` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/hicolor/24x24/apps/de.musiker15.sshcraft.png` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/hicolor/256x256/apps/de.musiker15.sshcraft.png` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/hicolor/32x32/apps/de.musiker15.sshcraft.png` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/hicolor/48x48/apps/de.musiker15.sshcraft.png` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/hicolor/512x512/apps/de.musiker15.sshcraft.png` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/hicolor/64x64/apps/de.musiker15.sshcraft.png` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/hicolor/scalable/apps/de.musiker15.sshcraft.svg` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/sshcraft-1024.png` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/sshcraft-macos-512.png` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/sshcraft.icns` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/sshcraft.ico` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/app-icon/sshcraft.svg` | SSHCraft contributors | Original, drawn for SSHCraft (`packaging/icons.sh` renders it from `sshcraft.svg`) | MIT OR Apache-2.0 | App icon |
| `assets/screenshots/forwarding.png` | SSHCraft contributors | Original screenshot of SSHCraft on sample data (`cargo run -p sshcraft-ui-egui --example snapshot -- forwarding.png --page forwarding --light --captions adwaita`) | MIT OR Apache-2.0 | Store screenshot: port forwarding |
| `assets/screenshots/hosts-dark.png` | SSHCraft contributors | Original screenshot of SSHCraft on sample data (`cargo run -p sshcraft-ui-egui --example snapshot -- hosts-dark.png --page hosts --dark --captions adwaita`) | MIT OR Apache-2.0 | Store screenshot: hosts, dark theme |
| `assets/screenshots/hosts.png` | SSHCraft contributors | Original screenshot of SSHCraft on sample data (`cargo run -p sshcraft-ui-egui --example snapshot -- hosts.png --page hosts --light --captions adwaita`) | MIT OR Apache-2.0 | Store screenshot: hosts |
| `assets/screenshots/snippets.png` | SSHCraft contributors | Original screenshot of SSHCraft on sample data (`cargo run -p sshcraft-ui-egui --example snapshot -- snippets.png --page snippets --light --captions adwaita`) | MIT OR Apache-2.0 | Store screenshot: snippets |
