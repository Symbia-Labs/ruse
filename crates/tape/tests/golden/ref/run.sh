#!/bin/sh
# Rebuilds the Swift reference from Bias's TapeDSP.swift and rewrites the golden files.
set -e
cd "$(dirname "$0")"
mkdir -p .build
export CLANG_MODULE_CACHE_PATH="$PWD/.build/modcache"
swiftc -O "${BIAS_TAPE_SWIFT:-$HOME/Repos/Bias/engine/Sources/BiasEngine/TapeDSP.swift}" stubs.swift main.swift -o .build/tape-ref
.build/tape-ref ..
