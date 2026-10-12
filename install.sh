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
