/*
 * demo.c — exercises the split-stack instrumentation.
 *
 * Design: the thread stack is a single continuous region.  The IR pass
 * inserts a check at every function entry; if SP - frame_estimate falls
 * below the soft bound, __morestack fires and the process aborts with a
 * diagnostic.  All functions here stay well within the thread stack, so
 * they should all complete successfully with no overflow.
 *
 *   - shallow():   simple call chain, no large locals.
 *   - big_frame(): allocates a 2 KiB local array.
 *   - recurse(20): recursive descent with a 256 B pad per frame (~5 KiB
 *                  total), well within any default thread stack.
 */

#include <stdint.h>
#include <stdio.h>

__attribute__((noinline))
static int leaf(int x) {
    return x ^ 0x55;
}

__attribute__((noinline))
static int shallow(int x) {
    return leaf(x) + leaf(x + 1);
}

__attribute__((noinline))
static int big_frame(int seed) {
    volatile char buf[2048];
    for (int i = 0; i < 2048; i++) buf[i] = (char)(seed + i);
    int s = 0;
    for (int i = 0; i < 2048; i++) s += buf[i];
    return s;
}

__attribute__((noinline))
static int recurse(int depth) {
    volatile int pad[64];   /* 256 B */
    for (int i = 0; i < 64; i++) pad[i] = depth + i;
    int sum = 0;
    for (int i = 0; i < 64; i++) sum += pad[i];
    if (depth <= 0) return sum;
    return recurse(depth - 1) + sum;
}

int main(void) {
    fprintf(stderr, "=== shallow ===\n");
    int a = shallow(7);

    fprintf(stderr, "=== big_frame ===\n");
    int b = big_frame(3);

    fprintf(stderr, "=== recurse(20) ===\n");
    int c = recurse(20);

    fprintf(stderr, "results: a=%d b=%d c=%d\n", a, b, c);
    fprintf(stderr, "[OK] all functions completed without overflow\n");
    return 0;
}
