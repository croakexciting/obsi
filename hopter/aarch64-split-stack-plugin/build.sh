#!/usr/bin/env bash
# build.sh — build the SplitStack pass plugin (host clang/LLVM 18).
set -euo pipefail
cd "$(dirname "$0")"

if [[ ! -d build ]]; then
    cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
fi
cmake --build build -j

echo
echo "Plugin built: $(ls -l build/SplitStackPass.so | awk '{print $9, "("$5" bytes)"}')"
