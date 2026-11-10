<p align="center">
  <img src="docs/icon.png" width="96" alt="AlexTerminal">
</p>

<h1 align="center">AlexTerminal</h1>

<p align="center">
  A Rust terminal for macOS and Linux with a vertical tab sidebar, tab groups,<br>
  up to four panes per tab, and workspace restoration after restart.
</p>

<p align="center">
  <img src="docs/screenshots/grid.png" alt="Tab group and 2×2 pane grid" width="900">
</p>

## Features

- **Vertical tab sidebar.** Tabs are organized into groups. A group with one tab takes up a single row.
- **Group views.** Show one tab at a time, stack all tabs in a group in one window (`⌘⇧G`), or tile them (`⌘⌥G`). Drag the dividers to resize stacked tabs. These views are also available in the group's context menu.
- **Up to four panes per tab.** Panes stack vertically. With four panes, use `⌘⇧M` to switch to a 2×2 grid. Resize panes with the mouse or keyboard.
- **Workspace restoration.** AlexTerminal saves windows, groups, tabs, working directories, and recent output. On restart, it offers to reconnect remote sessions.
- **Per-pane command history.** Each pane records its commands, working directories, and exit codes. The shell's Up arrow recalls commands from the current pane.
- **Tab status indicators.** See which tabs are running commands, connected over SSH, or have commands that finished in the background.
- **Shell integration for zsh, bash, and fish.** AlexTerminal enables integration automatically to track working directories, command start and completion, and SSH hosts.
- **Open the current directory.** Use `⌘⇧O` to open a pane's directory in Finder or your file manager, or `⌘⇧I` to open it in your editor.
- **Command palette.** Use `⌘⇧P` to search available actions. AlexTerminal also supports scrollback search, Command-clickable links, and tab profiles.
- **Themes and fonts.** Dusk and Linen provide dark and light themes with moderate contrast. AlexTerminal supports JetBrains Mono and can import Alacritty themes.
- **English and Russian interface.** The app follows your system language.

<p align="center">
  <img src="docs/screenshots/group.png" alt="All tabs in a group shown in one window" width="440">
  <img src="docs/screenshots/palette.png" alt="Command palette" width="440">
</p>

## Build from source

The build and app launch were verified on macOS 26 (Apple Silicon) with Rust 1.99. The Linux package lists follow iced and winit requirements. Building on a Linux system remains unverified.

### 1. Install prerequisites

#### macOS

Install the Xcode Command Line Tools if needed.

```bash
xcode-select --install
```

Install Rust using the official [rustup installer](https://rustup.rs) or Homebrew. If you use Homebrew, add rustup to your zsh path and reload it before selecting the stable toolchain.

```bash
brew install rustup
echo 'export PATH="$(brew --prefix rustup)/bin:$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
rustup default stable
```

#### Linux

Install the compiler and system libraries for windowing, keyboard input, and graphics.

**Debian and Ubuntu**

```bash
sudo apt install build-essential pkg-config curl git libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libvulkan-dev
```

**Fedora**

```bash
sudo dnf install gcc pkgconf-pkg-config curl git libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXrandr-devel libXi-devel vulkan-loader-devel
```

**Arch Linux**

```bash
sudo pacman -S base-devel curl git libxkbcommon wayland libx11 libxcursor libxrandr libxi vulkan-icd-loader
```

Install Rust with rustup.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Clone the repository

```bash
git clone https://github.com/psalexander/AlexTerminal.git
cd AlexTerminal
```

To use JetBrains Mono, run the optional script below to download it into `assets/fonts`. AlexTerminal can fall back to SF Mono, Menlo, or DejaVu Sans Mono.

```bash
scripts/fetch-fonts.sh
```

### 3. Build and run

Run a debug build directly from the source tree.

```bash
cargo run --bin alexterminal
```

Build and run the release binary. The first build downloads dependencies and may take several minutes.

```bash
cargo build --release
./target/release/alexterminal
```

The `alexterminal` binary includes the session server.

### 4. Install

#### macOS

The packaging script produces `dist/AlexTerminal.app` and `dist/AlexTerminal-<version>.dmg`.

```bash
packaging/macos/build-app.sh
```

Drag `AlexTerminal.app` into Applications or copy it from the command line.

```bash
cp -R dist/AlexTerminal.app /Applications/
```

macOS may flag a build without a Developer ID signature on first launch. You can sign the build using the instructions in step 6.

#### Linux

The packaging script builds a `.deb` with cargo-deb and an AppImage when `linuxdeploy` is available.

```bash
packaging/linux/build.sh
```

To install directly into your home directory, run these commands after building the release binary.

```bash
install -Dm755 target/release/alexterminal -t ~/.local/bin/
install -Dm644 packaging/linux/alexterminal.desktop -t ~/.local/share/applications/
install -Dm644 assets/icon-256.png ~/.local/share/icons/hicolor/256x256/apps/alexterminal.png
```

### 5. Run tests and checks

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

The tests run real shells in PTYs and start a session server in an isolated directory. GitHub Actions runs the same checks on macOS and Ubuntu. See the [CI workflow](.github/workflows/ci.yml).

### 6. Sign, notarize, and build a universal binary on macOS

| Environment variable | Purpose |
| --- | --- |
| `UNIVERSAL=1` | Builds arm64 and x86_64 binaries and combines them with `lipo` |
| `SIGN_IDENTITY` | Signs the binaries, app bundle, and disk image with hardened runtime |
| `NOTARY_PROFILE` | Submits the disk image for notarization and staples the ticket |

Create a notarization profile once.

```bash
xcrun notarytool store-credentials alexterminal-notary --apple-id you@example.com --team-id TEAMID
```

Build a signed, notarized release.

```bash
UNIVERSAL=1 SIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)" NOTARY_PROFILE=alexterminal-notary packaging/macos/build-app.sh
```

The release workflow for `v*` tags uses `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, `MACOS_SIGN_IDENTITY`, `NOTARY_API_KEY_P8`, `NOTARY_API_KEY_ID`, and `NOTARY_API_ISSUER`.

## Keyboard shortcuts

The table shows macOS defaults. Linux typically uses `Ctrl+Shift` for letter shortcuts and `Ctrl` for other keys. Search the command palette for the shortcuts on your platform. You can remap them in Settings.

| Action | macOS |
| --- | --- |
| New tab in a new group | `⌘T` |
| New tab in the current group | `⌘⇧T` |
| Toggle between one tab and all tabs in the group | `⌘⇧G` |
| Toggle between tiled and stacked group views | `⌘⌥G` |
| New pane | `⌘D` |
| Toggle between stacked panes and a 2×2 grid | `⌘⇧M` |
| Move between panes | `⌘⌥←↑↓→` |
| Close pane / close tab | `⌘W` / `⌘⇧W` |
| Reopen closed tab | `⌘⇧Z` |
| Select tab 1–9 | `⌘1…9` |
| Open directory in Finder | `⌘⇧O` |
| Show pane command history | `⌘⇧H` |
| Search scrollback | `⌘F` |
| Open command palette | `⌘⇧P` |
| Open Settings | `⌘,` |
| Quit | `⌘Q` |

## Settings and files

| Data | macOS | Linux |
| --- | --- | --- |
| Settings | `~/.config/alexterminal/config.toml` | `~/.config/alexterminal/config.toml` |
| Session snapshot, command history, and scrollback | `~/Library/Application Support/AlexTerminal/` | `~/.local/share/alexterminal/` |
| Logs | `~/Library/Logs/AlexTerminal/` | `~/.local/state/alexterminal/` |

The Settings window (`⌘,`) writes to `config.toml`. AlexTerminal reloads file edits while running. See section 5 of the [technical specification](SPEC.md) for the full list of settings.

## Session server

The session server runs in a thread inside the `alexterminal` process. Windows render the interface and forward input to the server. The server starts each pane's shell in its own PTY, processes output with `alacritty_terminal`, and keeps scrollback. It also tracks windows, groups, tabs, layouts, and sizes and saves them to `session.json`.

The server listens on a Unix socket accessible to its owner. Another invocation of `alexterminal` can open a tab in the running app, and `alexterminal list` reports its state.

- Quitting or closing the last window ends all shell processes. If a command is running, the app asks for confirmation.
- The server saves a snapshot before exit. On the next launch, AlexTerminal restores windows, tabs, working directories, and recent output, then starts fresh shells.
- When other windows remain open, closing a window detaches its tabs by default. You can restore them from the command palette while the app is running.

Before version 0.3, `alexterminal-server` ran as a separate background process. Version 0.3 integrates the server into the app, so server updates take effect when you restart the app. To keep a shell session alive after closing AlexTerminal, run `tmux` or `screen` inside a pane.

## Command line

```bash
alexterminal --working-directory ~/src/project
alexterminal --profile prod
alexterminal list
```

If a window is already open, invoking `alexterminal` with a working directory or profile adds a tab to the running app.

## Troubleshooting

- **Upgrading from a version earlier than 0.3.** AlexTerminal asks the legacy `alexterminal-server` to save its session, stops it, and restores the snapshot. Shells restart in the same directories. You can remove the old binary and any autostart entry at `~/Library/LaunchAgents/pro.odinai.alexterminal.server.plist` on macOS or `~/.config/systemd/user/alexterminal-server.service` on Linux.
- **Run an independent instance.** Set `ALEXTERMINAL_HOME` to store settings, data, logs, and the socket in a separate directory.

  ```bash
  ALEXTERMINAL_HOME=/tmp/at-test alexterminal
  ```

- **Enable debug logging.** Set the log level with `ALEXTERMINAL_LOG`.

  ```bash
  ALEXTERMINAL_LOG=debug alexterminal
  ```

## Known limitations

- On macOS, the native menu contains the standard application menu with Quit (`⌘Q`). Other actions are available through the command palette, keyboard shortcuts, and the tab context menu.
- Closing the last window quits the app and ends all sessions, including on macOS.
- The directory in a pane's title updates when the shell displays a prompt. If you run `cd` and a long command on the same line, the directory updates when that command finishes.
- After a crash, AlexTerminal restores the workspace layout and working directories. Recovered output is limited to the last autosave, which runs every 30 seconds by default.

## Project structure

| Crate | Purpose |
| --- | --- |
| `at-core` | Window, group, and tab models, configuration, themes, shortcuts, command history, and session snapshots |
| `at-proto` | Length-prefixed MessagePack protocol between the UI and the session server |
| `at-term` | Pane runtime with PTY, `alacritty_terminal` emulation, shell integration, input encoding, and process information |
| `at-daemon` | Session server that runs in a thread inside the app |
| `at-client` | Session server startup, discovery of running instances, and legacy server shutdown |
| `at-app` | iced 0.14 interface and the `alexterminal` binary |

Shell integration scripts are in `assets/shell/`. Translations are in `locales/en` and `locales/ru`. Requirements and design decisions are in the [technical specification](SPEC.md).

## License

AlexTerminal is available under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option. The workspace declares this as `MIT OR Apache-2.0` in [Cargo.toml](Cargo.toml).

JetBrains Mono is distributed separately under the SIL Open Font License 1.1.
