/*
 * runtime.c — stack-overflow-detection runtime for aarch64.
 *
 * Design: the thread stack remains a single continuous region (no stacklets).
 * At startup the soft bound is set to:
 *
 *     bound = initial_SP - max_stack_size + kGuardMargin
 *
 * Every instrumented function checks:
 *
 *     if (SP - frame_estimate < bound)  →  call __morestack
 *
 * __morestack tail-calls overflow_panic_entry (no SP switch).
 * overflow_panic_entry runs on the task stack and can trigger panic + unwind.
 *
 * Because the stack stays contiguous, the standard DWARF unwinder works
 * without any modification — panic / exception unwinding is unaffected.
 *
 * Max stack size defaults to 2 MB and can be overridden by:
 *
 *     HOPTER_MAX_STACK_KB=<kilobytes>
 *
 * Multi-thread support: pthread_create is wrapped so that each new thread
 * initialises its own TLS bound from its own SP before running user code.
 *
 * This file provides:
 *   __hopter_stklet_bound        — TLS variable checked by every instrumented fn
 *   __overflow_emergency_stack   — 64 KiB buffer; __morestack switches SP here
 *   __split_stack_ctor           — constructor: reads current SP and sets bound
 *   overflow_panic_entry         — weak noreturn: default aborts, override to panic+unwind
 *   pthread_create               — wrapper: initialises bound for new threads
 */

#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
#include <dlfcn.h>

/* ── Must match kFramePadding in SplitStackPass.cpp ──────────────────────
 * Covers: callee-save area (≤96 B on AArch64), spills, stack canary,
 * alignment gap, and the prologue timing-window safety buffer.           */
#define kFramePadding 128

/* ── Guard margin: space between the soft bound and the configured bottom.
 * Must be large enough for overflow_panic_entry + panic + unwinder to run.
 * 8 KiB is sufficient for panic! → begin_panic → _Unwind_RaiseException.  */
#define kGuardMargin (8 * 1024)

/* ── Default max stack size (bytes). Overridable via HOPTER_MAX_STACK_KB. */
#define kDefaultMaxStackBytes (2UL * 1024 * 1024)   /* 2 MiB */

/* ── TLS variable read by the IR pass ────────────────────────────────────
 * The IR pass uses General Dynamic TLS; __morestack (assembly) uses
 * Local Exec TLS — two ADD instructions from TPIDR_EL0, zero overhead. */
__thread uint64_t __hopter_stklet_bound = 0;

/* ── Emergency stack (kept for ABI compatibility, no longer used by __morestack) */
char __overflow_emergency_stack[65536] __attribute__((aligned(16)));
/* ── Diagnostics (set at init) ────────────────────────────────────────── */
static uint64_t s_init_sp;
static uint64_t s_max_stack_bytes;

/* ── Constructor ──────────────────────────────────────────────────────── */
__attribute__((constructor))
static void __split_stack_ctor(void) {
    uint64_t sp;
    __asm__ volatile("mov %0, sp" : "=r"(sp));
    s_init_sp = sp;

    /* Allow override via environment variable. */
    s_max_stack_bytes = kDefaultMaxStackBytes;
    const char *env = getenv("HOPTER_MAX_STACK_KB");
    if (env && *env) {
        unsigned long kb = strtoul(env, NULL, 10);
        if (kb > 0) s_max_stack_bytes = kb * 1024UL;
    }

    /* Soft bound = init_SP - max_stack + kGuardMargin.
     * The guard margin ensures overflow_panic_entry has enough stack to run.
     * kFramePadding is already baked into every frame estimate.          */
    __hopter_stklet_bound = sp - s_max_stack_bytes + kGuardMargin;

    fprintf(stderr,
            "[hopter] init SP=0x%lx  max_stack=%lu KB\n"
            "[hopter] bound=0x%lx  (SP - max_stack + %d B guard)\n",
            (unsigned long)sp,
            (unsigned long)(s_max_stack_bytes / 1024),
            (unsigned long)__hopter_stklet_bound,
            kGuardMargin);
}

/* ── overflow_panic_entry ─────────────────────────────────────────────────
 * Tail-called by __morestack (assembly) with SP on the task stack.
 * Runs inside the kGuardMargin region — plenty of space for panic.
 *
 * This is a WEAK symbol.  Override it with a strong definition to get
 * real stack unwinding instead of process termination.  Typical override
 * in Rust (with std or the 'unwinding' crate):
 *
 *     #[no_mangle]
 *     pub extern "C-unwind" fn overflow_panic_entry() -> ! {
 *         panic!("stack overflow");
 *     }
 *
 * Because __morestack uses 'b' (tail call), overflow_panic_entry's
 * .eh_frame shows the instrumented function as its caller.  Any unwinder
 * invoked from here will walk the full task stack correctly.            */
__attribute__((weak, noreturn))
void overflow_panic_entry(void) {
    fprintf(stderr,
            "\n[hopter] *** STACK OVERFLOW ***\n"
            "[hopter]   bound=0x%lx  init_SP=0x%lx  max=%lu KB\n"
            "[hopter]   Override 'overflow_panic_entry' to unwind instead of abort.\n",
            (unsigned long)__hopter_stklet_bound,
            (unsigned long)s_init_sp,
            (unsigned long)(s_max_stack_bytes / 1024));
    abort();
}

/* ── pthread_create wrapper ───────────────────────────────────────────────
 * Each new thread must initialise its own TLS __hopter_stklet_bound from
 * its own SP before any instrumented function runs.  We wrap pthread_create
 * with a trampoline that does this as the very first thing in the new thread.
 *
 * dlsym(RTLD_NEXT) locates the real pthread_create from libpthread so we
 * don't recurse.                                                          */

typedef struct {
    void *(*fn)(void *);
    void *arg;
} thread_arg_t;

static void *thread_trampoline(void *p) {
    thread_arg_t *ta = (thread_arg_t *)p;
    void *(*fn)(void *) = ta->fn;
    void *arg           = ta->arg;
    free(ta);

    /* Initialise this thread's soft bound from its own SP. */
    uint64_t sp;
    __asm__ volatile("mov %0, sp" : "=r"(sp));
    __hopter_stklet_bound = sp - s_max_stack_bytes + kGuardMargin;

    return fn(arg);
}

int pthread_create(pthread_t *thread, const pthread_attr_t *attr,
                   void *(*start_routine)(void *), void *arg) {
    static int (*real_pthread_create)(pthread_t *, const pthread_attr_t *,
                                      void *(*)(void *), void *) = NULL;
    if (!real_pthread_create)
        real_pthread_create = dlsym(RTLD_NEXT, "pthread_create");

    thread_arg_t *ta = malloc(sizeof(*ta));
    if (!ta) return -1;
    ta->fn  = start_routine;
    ta->arg = arg;
    return real_pthread_create(thread, attr, thread_trampoline, ta);
}
