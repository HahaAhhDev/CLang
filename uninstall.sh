#!/bin/sh
set -e
PREFIX="${PREFIX:-/usr/local/bin}"
rm -f "$PREFIX/clng" "$PREFIX/secular"
rm -f "$HOME/.config/micro/syntax/clang.yaml"
rm -f "$HOME/.nano/clang.nanorc"
rm -rf "$HOME/.vscode/extensions/clang-0.6.0"
echo "removed clng and secular from $PREFIX"
