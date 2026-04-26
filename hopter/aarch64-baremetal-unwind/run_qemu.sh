#!/bin/bash
set -e
# 构建
cargo build
# QEMU 运行
qemu-system-aarch64 \
  -M virt \
  -cpu cortex-a53 \
  -nographic \
  -kernel target/aarch64-unknown-none/debug/aarch64-baremetal-unwind \
  -monitor none \
  -serial stdio \
  -smp 1 \
  -m 128M
