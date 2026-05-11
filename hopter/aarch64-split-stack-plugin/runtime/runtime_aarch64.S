/* runtime_aarch64.S — __morestack: detect stack overflow and abort.
 *
 * Called by every instrumented function when:
 *
 *     SP - frame_estimate < __hopter_stklet_bound
 *
 * This means the function's frame would reach into the guard zone.
 * The response is to abort with a diagnostic — the stack is continuous,
 * there are no stacklets to allocate.
 *
 * DESIGN CONSTRAINT
 * -----------------
 * __morestack is called BEFORE the function's own alloca has run.
 * At entry:
 *
 *   SP = caller_sp - prologue_delta   (callee-save pushes only, e.g. -32)
 *   The region below SP is below the soft bound but may not be past the
 *   real stack bottom — we still have kGuardMargin bytes of real stack.
 *
 * Therefore __morestack MUST NOT (before the SP switch):
 *   - Write below the current SP
 *   - Make any function call (bl requires writing x30 then the callee
 *     may push a frame — we don't know how deep the real stack is yet)
 *   - Touch x19–x28 (callee-saved; caller holds live values)
 *   - Touch x29 (caller's FP)
 *
 * We MAY clobber x30 after the SP switch because we will never return.
 *
 * WHAT WE DO
 * ----------
 * 1. Switch SP to __overflow_emergency_stack (a 4 KiB static buffer
 *    in runtime.c, always safe to use).
 * 2. Call __morestack_overflow() via bl — now on the emergency stack,
 *    this is safe.  __morestack_overflow never returns.
 *
 * REGISTERS USED (x0/x1 are caller-saved — no saves needed)
 * ----------------------------------------------------------
 *   x1  = address of __overflow_emergency_stack
 */

    .text

    .global __morestack
    .type   __morestack, %function
__morestack:

#ifdef HOPTER_DIAG_SP
    /* == [diag] Save real SP before switching ============================
     * Pure register ops — no stack writes.                               */
    adrp    x1, __morestack_overflow_sp
    add     x1, x1, #:lo12:__morestack_overflow_sp
    mov     x2, sp
    str     x2, [x1]
#endif

    /* == Switch SP to the static emergency stack =========================
     *
     * __overflow_emergency_stack is a 4096-byte, 16-byte-aligned static
     * buffer defined in runtime.c.  We point SP to its top so that
     * __morestack_overflow can safely call fprintf and abort.
     *
     * We have not written below the old SP yet, so this SP switch is
     * the very first write to any stack memory.                          */
    adrp    x1, __overflow_emergency_stack
    add     x1, x1, #:lo12:__overflow_emergency_stack
    add     x1, x1, #65536          /* SP = top of the 64 KiB buffer     */
    mov     sp, x1

    /* == Call C overflow handler (never returns) ========================= */
    bl      __morestack_overflow

    /* Unreachable; silence assembler warnings. */
0:  b       0b

    .size   __morestack, . - __morestack


    /* Mark stack non-executable. */
    .section .note.GNU-stack, "", %progbits
