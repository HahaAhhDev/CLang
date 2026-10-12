#!/bin/sh
set -e
PREFIX="${PREFIX:-/usr/local/bin}"
rm -f "$PREFIX/clng" "$PREFIX/secular"
echo "removed clng and secular from $PREFIX"
