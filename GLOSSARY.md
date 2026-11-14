# Glossary

| Term | Meaning |
|---|---|
| **Location** | Where a host or group lives: "only on this device" or a particular sync server. Entries can move between locations; the sidebar shows all of them with a mark for where an entry comes from. |
| **Group** | A collection of hosts, often one per server with several host entries for different users and keys. Settings on a group (address, port and so on) apply to its hosts unless a host overrides them. Groups nest. |
| **Host** | Address, port, user, key and display name; values can be inherited from the group. |
| **Identity** | A reusable bundle of user, password and/or key for hosts and groups. |
| **Key** | An SSH key from one of three sources: the Proton Pass SSH agent, Proton Pass through `pass-cli`, or SSHCraft's own encrypted vault. |
| **Vault** | SSHCraft's local store of secrets, encrypted with XChaCha20-Poly1305 under a master key kept in the operating system's credential store. |
| **Account** | A user on a sync server, with name, email and account key. The account key is created by the client and kept in Proton Pass. |
| **Device** | One installation of SSHCraft signed into an account, with its own device key; it can be revoked on its own. |
| **Area** | A set of synced records that share one area key: the personal area of an account, or a share. |
| **Share** | A host or group shared with other accounts on the same server, with or without keys, read-only or read-write. |
| **Workspace** | Several sessions side by side (split) or as a main view with a sidebar (focus), with broadcast input. |
| **Session log** | A recording of a session's output (never its input), with bookmarks and comments. |
| **Agent gate** | The check every request of an AI agent passes: rights per category, hidden hosts, audit log, confirmation in the window. |
| **Craft apps** | The family of Rust desktop apps by storytold (GridCraft, PdfCraft, PhotoCraft and others) whose architecture and conventions SSHCraft follows. |
