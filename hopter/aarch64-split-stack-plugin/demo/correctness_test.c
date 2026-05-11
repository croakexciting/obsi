/*
 * correctness_test.c — verify that the IR-inserted stack checks do NOT
 * interfere with correct computation (no false positives).
 *
 * All functions here fit comfortably within the thread stack.  The checks
 * fire (comparing SP against the soft bound) but always pass.  If any
 * check ever triggers __morestack the process will abort — that would be
 * a false-positive bug in the pass or the runtime.
 *
 * Tests:
 *   1. large_frame  — 900 B volatile local; checks frame estimate accuracy.
 *   2. deep_recurse — 30 levels × ~272 B/frame ≈ 8 KiB; checks recursion.
 *   3. chain        — large_frame + deep_recurse in sequence.
 *   4. vla_test     — VLA (dynamic alloca); checks per-alloca instrumentation.
 */

#include <stdint.h>
#include <stdio.h>

/* ── Test 1: large static local ─────────────────────────────────────────── */
#define BUF_SIZE 900
#define EXPECTED_CHECKSUM 106566u   /* sum(i % 256, i=0..899) */

__attribute__((noinline))
static unsigned large_frame(void) {
    volatile unsigned char buf[BUF_SIZE];
    for (int i = 0; i < BUF_SIZE; i++) buf[i] = (unsigned char)i;
    unsigned sum = 0;
    for (int i = 0; i < BUF_SIZE; i++) sum += buf[i];
    return sum;
}

/* ── Test 2: deep recursion ──────────────────────────────────────────────── */
/* 30 levels × ~272 B/frame ≈ 8 KiB — well within any default thread stack. */
__attribute__((noinline))
static int deep_recurse(int depth) {
    volatile int pad[64];           /* 256 B */
    for (int i = 0; i < 64; i++) pad[i] = depth + i;
    int s = 0;
    for (int i = 0; i < 64; i++) s += pad[i];
    if (depth <= 0) return s - (63 * 64 / 2);
    return deep_recurse(depth - 1) + depth;
}

/* ── Test 3: chain ───────────────────────────────────────────────────────── */
__attribute__((noinline))
static int chain(void) {
    unsigned a = large_frame();
    int      b = deep_recurse(29);
    return (int)(a ^ (unsigned)b);
}

/* ── Test 4: VLA ─────────────────────────────────────────────────────────── */
/* The pass must emit a per-alloca check for dynamic-sized allocations.
 * vla_test(900) uses ~900 B dynamically — the per-alloca check should pass
 * because the thread stack has plenty of room.
 * Expected result: n (each element set to 1, so sum == n).               */
__attribute__((noinline))
static unsigned vla_test(int n) {
    volatile unsigned char buf[n];
    for (int i = 0; i < n; i++) buf[i] = 1;
    unsigned sum = 0;
    for (int i = 0; i < n; i++) sum += buf[i];
    return sum;
}

/* ── Main ────────────────────────────────────────────────────────────────── */
int main(void) {
    int pass = 1;

    fprintf(stderr, "\n=== Correctness test ===\n");

    /* Test 1 */
    unsigned r1 = large_frame();
    if (r1 == EXPECTED_CHECKSUM) {
        fprintf(stderr, "[PASS] large_frame: result=%u (expected %u)\n",
                r1, EXPECTED_CHECKSUM);
    } else {
        fprintf(stderr, "[FAIL] large_frame: result=%u  expected=%u\n",
                r1, EXPECTED_CHECKSUM);
        pass = 0;
    }

    /* Test 2 */
    int r2 = deep_recurse(29);
    if (r2 == 435) {
        fprintf(stderr, "[PASS] deep_recurse(29): result=%d (expected 435)\n", r2);
    } else {
        fprintf(stderr, "[FAIL] deep_recurse(29): result=%d  expected=435\n", r2);
        pass = 0;
    }

    /* Test 3 */
    int r3 = chain();
    fprintf(stderr, "[INFO] chain: result=%d\n", r3);

    /* Test 4: VLA */
    unsigned r4 = vla_test(900);
    if (r4 == 900u) {
        fprintf(stderr, "[PASS] vla_test(900): result=%u (expected 900)\n", r4);
    } else {
        fprintf(stderr, "[FAIL] vla_test(900): result=%u  expected=900\n", r4);
        pass = 0;
    }

    if (pass) {
        fprintf(stderr, "\n[OK] all checks passed — no false positives\n");
        return 0;
    } else {
        fprintf(stderr, "\n[FAIL] some tests failed\n");
        return 1;
    }
}
