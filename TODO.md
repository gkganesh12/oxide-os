# Technical Debt & Known Issues

## Critical (fix before next phase)

- [ ] **Stack memory leak on task kill** — `exit_current()` drops the Task struct but never returns the 4 physical frames to the frame allocator. Every killed task permanently leaks 16 KiB.
- [ ] **Timer interval not calibrated** — `0x20000` is arbitrary. Actual frequency depends on bus clock. Should calibrate against a known time source (PIT or TSC) at boot.
- [ ] **Cooperative preemption only** — tasks must call `should_reschedule()` + `yield_now()` in their loop. A task that never checks (infinite compute, no yield point) cannot be preempted. True preemption requires interrupt-return patching or a dedicated preemption mechanism.

## Important (fix soon)

- [ ] **No task ID recycling** — IDs monotonically increase. After 256 spawns+kills, MAX_TASKS is exhausted even if only 2 tasks are alive.
- [ ] **No SMP support** — single-core only. APIC IDs, per-CPU scheduler state, and atomic context switch needed for multi-core.
- [ ] **Frame allocator is O(n) per allocation** — linear bitmap scan. Fine for <32K frames but will be slow with more RAM. Consider buddy allocator.
- [ ] **No unmap/free for page tables** — `alloc_and_map` allocates but there's no `unmap_and_free`. Dead task stacks remain mapped forever.
- [ ] **Heap cannot grow** — fixed 1 MiB. If exhausted, kernel panics. Should support dynamic heap expansion.

## Nice to Have (Phase 3+)

- [ ] Time-slice accounting (RT tasks get more ticks per quantum)
- [ ] Priority inheritance (prevent priority inversion)
- [ ] Per-task CPU time tracking (for resource accounting)
- [ ] Kernel profiling (where time is spent)
- [ ] Serial input (not just output) for interactive debugging
- [ ] Structured kernel logging with log levels (debug/info/warn/error)

## Resolved

- [x] ~~Non-contiguous stack allocation~~ — Fixed: virtual mapping
- [x] ~~Interrupt frame leak~~ — Fixed: deferred switch model
- [x] ~~Deadlock in panic handler~~ — Fixed: panic_println bypasses lock
- [x] ~~static mut in GDT~~ — Fixed: safe static Stack wrapper
- [x] ~~static mut in APIC~~ — Fixed: AtomicU64
- [x] ~~Kernel memory not protected~~ — Fixed: mark kernel/bootloader regions used
- [x] ~~No OOM handler~~ — Fixed: alloc_error_handler
- [x] ~~Task monopolization~~ — Fixed: fair pick_next_fair
- [x] ~~Interrupts disabled on fresh tasks~~ — Fixed: entry trampoline
