# Path of Building (Rust)

A fast, cross-platform rewrite of [Path of Building Community](https://github.com/PathOfBuildingCommunity/PathOfBuilding), the community fork of Path of Building, the build planner for Path of Exile, in Rust: the calculation engine and the whole desktop interface.

**This is not a fork.** No Lua runs in it and it does not share the original's code base or history: the engine and the UI were rewritten from scratch. Its logic, however, is a faithful port of the original's, and it uses the original's data (see [Credits](#credits-and-licenses)). All the credit for the planner, its calculations and its data belongs to the Path of Building Community.

It is unofficial and not affiliated with Path of Building Community or Grinding Gear Games.

## Why use it

- **Faster.** See the numbers below.
- **Less memory.** The peak is about 40% lower.
- **Runs on more systems.** Windows, macOS and Linux, natively, without Wine.
- **Same numbers as the original.** 426 reference builds match the original bit for bit, on about 320,000 values, on all three systems.
- **Same interface.** All the tabs: Tree, Skills, Items, Calcs, Config, Notes, Import/Export, Party, Compare, Trade. On Windows, screens are compared with recordings of the original, pixel by pixel.
- **Your builds stay safe.** It keeps its data in its own folder and never changes the original's.

## Speed and memory

Measured on one Windows machine with the same builds. Your numbers will differ.

| | This app | Original |
|---|---|---|
| Power calculation | ~2.3 s | ~15 s |
| Opening the Items tab | 2.3 ms | 10 ms |
| Recalculation after equipping an item | 51 ms | 77 ms |
| Memory in steady use | 363–389 MB | 446–491 MB |
| Peak memory | 461 MB | 757 MB |

Not everything is faster: the first hover over an item takes 12–15 ms against 8–12 ms in the original.

## Platforms

| System | State |
|---|---|
| Windows x64 | Installer and zip. Tested. |
| Linux x64 | Archive, glibc 2.28 or newer (Ubuntu 20.04+, Debian 10+, RHEL 8+). Tested on Ubuntu 24.04 with KDE Plasma on Wayland and XWayland. Other desktops and a plain X11 session are not tested. |
| macOS on Apple Silicon | Archive, not signed. Tested on an M1. Intel Macs are not tested. |

## Install

Downloads are on the [Releases](../../releases) page. The files are not signed, so your system may warn you.

**Windows.** Run `pob-rust-<version>-windows-x64-setup.exe`. It needs no administrator rights and installs over an older version. It can also open `pob://` links, and gives them back to the original on uninstall. Or unpack the zip anywhere and run `Path of Building (Rust).exe`, keeping the files next to it.

**Linux.** Unpack the archive and run its installer. It needs no `sudo` and adds the app to your applications menu and a `pob-rust` command:

```bash
tar -xzf pob-rust-<version>-linux-x64.tar.gz
cd pob-rust-<version>-linux-x64
./install.sh
```

Run a newer `install.sh` to update, and `~/.local/share/pob-rust/install.sh --uninstall` to remove it; neither touches your builds. `./install.sh --pob-links` also makes the app open `pob://` links. Without installing, you can also run `./"Path of Building (Rust)"` right from the unpacked folder. You need a Vulkan or OpenGL driver, and `libxkbcommon-x11` on an X11 session.

**macOS.** Unpack `pob-rust-<version>-macos-arm64.tar.gz` and run `Path of Building (Rust)` from the folder, keeping the files next to it. There is no `.app` yet, and as the archive is not signed, macOS may refuse to open it until you allow it in System Settings > Privacy & Security. Cmd works as Ctrl. Cmd+Q asks about unsaved changes, as closing the window does, and the standard app menu with Hide is not there for now.

Builds and settings live in `Documents/Path of Building (Rust)`.

## Known limitations

- No automatic updates: download new versions by hand.
- Game data updates arrive with new releases of this app, not on their own.
- On Linux and macOS, `sin` and `pow` of the system's math library can differ from Windows in the last bit. The 426 reference builds are not affected, but a tree node's position is, far below what can be seen.
- Tree images can differ from the original by a few pixels, because the order of `pairs` in the original differs from run to run.
- On macOS, stretched UI sprites can differ slightly from the original: the GPU filters them differently. The cause is not fully proven yet.

## Building from source

You need the Rust version in `rust-toolchain.toml`. The game data and art are not in this repository, so take them from a release package:

```bash
cargo build --release -p pob-app
```

Then copy `target/release/pob-app` into an unpacked release package and run it there. `tools/package-unix.sh` makes a package for Linux or macOS, and `tools/package.ps1` with `tools/installer.ps1` for Windows.

## Credits and licenses

- **Path of Building Community**, the community fork of Path of Building. Copyright (c) 2016 David Gowor, under the MIT license: see [LICENSE-PathOfBuilding.md](LICENSE-PathOfBuilding.md). This project ports its logic. Next to the program, packages carry its unchanged fonts, tree data and assets, and data converted from it, with the same license file.
- **Game data and art** © Grinding Gear Games.
- **The code of this project** is under the MIT license: see [LICENSE](LICENSE).
