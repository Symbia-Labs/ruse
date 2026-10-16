#!/bin/sh
# Cross-checks RUSE's CLAP layouts and a built plugin against the real CLAP header.
#
#   crates/clap/tests/abi/run.sh [PATH-TO-LIBRARY]
#
# It downloads the header once into target/ref, prints the struct sizes and offsets (the numbers in
# tests/abi.rs), and, given a library such as "Warm Saturator.clap/Contents/MacOS/Warm Saturator", loads
# it with a host written in C against that header.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../.." && pwd)
ref="$root/target/ref"
mkdir -p "$ref"
if [ ! -d "$ref/clap-main/include/clap" ]; then
   curl -sSL -o "$ref/clap.tar.gz" https://codeload.github.com/free-audio/clap/tar.gz/refs/heads/main
   tar xzf "$ref/clap.tar.gz" -C "$ref"
fi
cc -I "$ref/clap-main/include" "$here/layout.c" -o "$ref/layout"
"$ref/layout"
if [ -n "$1" ]; then
   cc -I "$ref/clap-main/include" "$here/host.c" -o "$ref/host" -lm
   "$ref/host" "$1"
fi
