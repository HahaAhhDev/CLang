#!/bin/sh
set -e
PREFIX="${PREFIX:-/usr/local/bin}"

cargo build --release
mkdir -p "$PREFIX"
cp target/release/clng "$PREFIX/clng"
chmod +x "$PREFIX/clng"
cp tools/secular "$PREFIX/secular"
chmod +x "$PREFIX/secular"
echo "installed clng to $PREFIX/clng"
echo "installed secular to $PREFIX/secular"
"$PREFIX/clng" examples/hello.clng
"$PREFIX/secular"

HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HOME/.config/micro/syntax"
cp "$HERE/editors/micro/syntax/clang.yaml" "$HOME/.config/micro/syntax/clang.yaml"
echo "installed micro coloring for .clng files"
mkdir -p "$HOME/.nano"
cp "$HERE/editors/nano/clang.nanorc" "$HOME/.nano/clang.nanorc"
if ! grep -qs "clang.nanorc" "$HOME/.nanorc" 2>/dev/null; then
  echo "include \"$HOME/.nano/clang.nanorc\"" >> "$HOME/.nanorc"
fi
echo "installed nano coloring for .clng files"
if [ -d "$HOME/.vscode/extensions" ]; then
  rm -rf "$HOME/.vscode/extensions/clang-0.6.0"
  cp -r "$HERE/editors/vscode" "$HOME/.vscode/extensions/clang-0.6.0"
  echo "installed vscode extension sources"
else
  echo "vscode extensions dir missing, skipped (see editors/vscode/README.md)"
fi
