#!/usr/bin/env bash
# run.sh — cross-compile the demos for aarch64-linux-gnu, run under qemu,
# and dump the disassembly of one instrumented function for inspection.
#
# Three binaries are built:
#   build/demo             — normal execution, no overflow (expected exit 0)
#   build/correctness_test — correctness checks, no overflow (expected exit 0)
#   build/overflow_demo    — intentional overflow, aborts (expected exit != 0)
set -euo pipefail
cd "$(dirname "$0")"

PLUGIN="$PWD/build/SplitStackPass.so"
if [[ ! -f "$PLUGIN" ]]; then
    echo "Plugin not built; running ./build.sh first..."
    ./build.sh
fi

OUT=build/demo
OUT_CORRECT=build/correctness_test
OUT_OVERFLOW=build/overflow_demo
SYSROOT=/usr/aarch64-linux-gnu
TARGET=aarch64-linux-gnu

mkdir -p build

# Base clang flags for cross-compilation.
# Compilation uses --sysroot to locate aarch64 headers.
# Linking does NOT use --sysroot: both lld and aarch64-linux-gnu-ld prepend
# the sysroot prefix to absolute paths found inside linker scripts (libc.so
# is a GNU ld script referencing /usr/aarch64-linux-gnu/lib/libc.so.6), which
# creates a double-prefix path that cannot be resolved.  Instead we specify
# -B (for crt files) and -L (for libraries) explicitly.
CLANG_COMPILE=(
    clang
    --target=$TARGET
    --sysroot=$SYSROOT
)

CLANG_LINK=(
    clang
    --target=$TARGET
    -fuse-ld=/usr/bin/aarch64-linux-gnu-ld
    -B$SYSROOT/lib
    -L$SYSROOT/lib
)

CFLAGS=(
    --target=$TARGET
    --sysroot=$SYSROOT
    -O2
    -g
    -fpass-plugin="$PLUGIN"
)

RUNTIME_OBJS=(build/runtime.o build/runtime_aarch64.o)

# ── Build runtime (NOT instrumented — the runtime must not check itself) ──
"${CLANG_COMPILE[@]}" -O2 -g \
    -c runtime/runtime.c -o build/runtime.o
"${CLANG_COMPILE[@]}" \
    -c runtime/runtime_aarch64.S -o build/runtime_aarch64.o

# ── Build and run main demo ────────────────────────────────────────────────
clang "${CFLAGS[@]}" -c demo/demo.c -o build/demo.o
"${CLANG_LINK[@]}" build/demo.o "${RUNTIME_OBJS[@]}" -lpthread -ldl -o "$OUT"

echo
echo "=== Running main demo under qemu-aarch64 ==="
qemu-aarch64 -L $SYSROOT "$OUT"

echo
echo "=== Disassembly of \`recurse\` (shows instrumented prologue) ==="
llvm-objdump -d --no-show-raw-insn -M no-aliases "$OUT" \
  | awk '/<recurse>:/{flag=1} flag{print} /^$/{if(flag)exit}' || true

# ── Build and run correctness test ────────────────────────────────────────
echo
echo "=== Correctness test: verify no false positives ==="
clang "${CFLAGS[@]}" \
    -c demo/correctness_test.c -o build/correctness_test.o

"${CLANG_LINK[@]}" \
    build/correctness_test.o "${RUNTIME_OBJS[@]}" \
    -lpthread -ldl \
    -o "$OUT_CORRECT"

qemu-aarch64 -L $SYSROOT "$OUT_CORRECT"

# ── Build and run overflow demo (expected to abort) ────────────────────────
echo
echo "=== Overflow demo: intentional stack overflow (expect abort) ==="
clang "${CFLAGS[@]}" \
    -c demo/overflow_demo.c -o build/overflow_demo.o

"${CLANG_LINK[@]}" \
    build/overflow_demo.o "${RUNTIME_OBJS[@]}" \
    -lpthread -ldl \
    -o "$OUT_OVERFLOW"

# Run and expect a non-zero exit code (abort = overflow detected correctly).
if qemu-aarch64 -L $SYSROOT "$OUT_OVERFLOW"; then
    echo "[FAIL] overflow_demo exited 0 — overflow was NOT detected!"
    exit 1
else
    echo "[OK] overflow_demo aborted as expected — overflow detection works"
fi
