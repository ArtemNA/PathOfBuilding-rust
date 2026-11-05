#!/bin/bash
# In the Linux package as install.sh. Installs for this user, without sudo: the files, the `pob-rust` command and an entry in the applications menu.
#   ./install.sh               install, or update over an older version
#   ./install.sh --pob-links   also open pob:// links in this application
#   ./install.sh --uninstall   remove all of it
# Builds and settings are in Documents/Path of Building (Rust): neither updating nor removing touches them.
set -euo pipefail

data=${XDG_DATA_HOME:-$HOME/.local/share}
dir=$data/pob-rust
cmd=$HOME/.local/bin/pob-rust
menu=$data/applications/pob-rust.desktop
exe="Path of Building (Rust)"

refresh() {
    if command -v update-desktop-database >/dev/null; then update-desktop-database -q "$data/applications" || true; fi
}

links=
case "${1:-}" in
    "") ;;
    --pob-links) links=1 ;;
    --uninstall)
        if [ -f "$dir/data.lock" ]; then rm -rf "$dir"; fi
        if [ -L "$cmd" ]; then rm -f "$cmd"; fi
        rm -f "$menu"
        refresh
        echo "Removed. Your builds are still in Documents/Path of Building (Rust)."
        exit 0
        ;;
    *) echo "usage: $0 [--pob-links | --uninstall]" >&2; exit 2 ;;
esac

src=$(cd "$(dirname "$0")" && pwd)
[ -f "$src/data.lock" ] && [ -x "$src/$exe" ] || { echo "run this from the unpacked package" >&2; exit 1; }
if [ -e "$dir" ] && [ ! -f "$dir/data.lock" ]; then echo "$dir exists and is not this application: nothing changed" >&2; exit 1; fi

if [ "$src" != "$dir" ]; then
    mkdir -p "$data"
    tmp=$(mktemp -d "$data/pob-rust.XXXXXX")
    cp -R "$src/." "$tmp/"
    chmod 755 "$tmp"
    rm -rf "$dir"
    mv "$tmp" "$dir"
fi
mkdir -p "$(dirname "$cmd")" "$(dirname "$menu")"
ln -sfn "$dir/$exe" "$cmd"
{
    echo "[Desktop Entry]"
    echo "Type=Application"
    echo "Name=Path of Building (Rust)"
    echo "Comment=Build planner for Path of Exile"
    echo "Exec=\"$cmd\" %u"
    echo "Icon=$dir/pob-rust.png"
    echo "Categories=Game;Utility;"
    echo "StartupWMClass=pob-rust"
    if [ -n "$links" ]; then echo "MimeType=x-scheme-handler/pob;"; fi
} > "$menu"
refresh
if [ -n "$links" ] && command -v xdg-mime >/dev/null; then xdg-mime default pob-rust.desktop x-scheme-handler/pob; fi

echo "Installed: find Path of Building (Rust) in the applications menu, or run: $cmd"
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) echo "Note: $HOME/.local/bin is not in PATH, so the short command pob-rust needs the full path." ;; esac
