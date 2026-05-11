/*
 * overflow_demo.c — demonstrate stack overflow detection.
 *
 * This binary deliberately recurses until the soft bound is reached.
 * __morestack fires, prints a diagnostic, and calls abort().
 *
 * Expected behaviour: the process exits with a non-zero status and
 * prints "[hopter] *** STACK OVERFLOW DETECTED ***" to stderr.
 * run.sh treats a non-zero exit as the expected (correct) outcome.
 *
 * Each frame fills a 4 KiB volatile buffer with a loop (so the compiler
 * sees a non-constant index GEP and keeps the full alloca intact — LLVM
 * SROA would otherwise split constant-index accesses into separate small
 * allocas).  buf[0] is read AFTER the recursive call to prevent tail-call
 * optimisation.
 *
 * With the default 2 MB limit and ~4 KiB per frame, overflow fires after
 * approximately 490 recursive calls.
 */

#include <stdio.h>

__attribute__((noinline))
static int deep(int n) {
    /* Fill every element via a variable index: this forces the IR to keep
     * a single 4096-byte alloca (LLVM SROA cannot split variable-index GEPs).
     */
    volatile char buf[4096];
    for (int i = 0; i < 4096; i++) buf[i] = (char)n;

    if (n % 50 == 0)
        fprintf(stderr, "[overflow_demo] depth=%d\n", n);

    int r = deep(n + 1);

    /* Read buf[0] AFTER the recursive call to prevent tail-call opt. */
    return r + (int)buf[0];
}

int main(void) {
    fprintf(stderr, "[overflow_demo] starting deep recursion (4 KiB/frame)...\n");
    fprintf(stderr, "[overflow_demo] expect overflow after ~490 frames\n");
    deep(0);
    /* Unreachable. */
    return 0;
}
