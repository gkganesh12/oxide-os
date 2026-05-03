# Technical Debt & Known Issues

## Critical (fix before next phase)

- [ ] **Cooperative preemption only** — tasks must call `should_reschedule()` + `yield_now()` in their loop. A task that never checks (infinite compute, no yield point) cannot be preempted. True preemption requires interrupt-return patching or a dedicated preemption mechanism.
- [ ] **Timer interval not calibrated** — `0x20000` is arbitrary. Actual frequency depends on bus clock. Should calibrate against a known time source (PIT or TSC) at boot.

## Important (fix soon)

- [ ] **No SMP support** — single-core only. APIC IDs, per-CPU scheduler state, and atomic context switch needed for multi-core.
- [ ] **Frame allocator is O(n) per allocation** — linear bitmap scan. Fine for <32K frames but will be slow with more RAM. Consider buddy allocator.
- [ ] **No unmap/free for page tables** — `alloc_and_map` allocates but there's no `unmap_and_free`. Dead task stacks remain mapped forever in virtual space.
- [ ] **Heap cannot grow** — fixed 1 MiB. If exhausted, kernel panics. Should support dynamic heap expansion.
- [ ] **Shared memory not truly contiguous** — `create_region` allocates frames one-by-one. Physical frames may not be contiguous. Fine for HHDM-mapped access but problematic for DMA.

## Nice to Have (future phases)

- [ ] Time-slice accounting (RT tasks get more ticks per quantum)
- [ ] Priority inheritance (prevent priority inversion)
- [ ] Per-task CPU time tracking (for resource accounting)
- [ ] Kernel profiling (where time is spent)
- [ ] Serial input (not just output) for interactive debugging
- [ ] Structured kernel logging with log levels (debug/info/warn/error)
- [ ] Suppress dead-code warnings for public API modules (allow(dead_code) on ipc/capability mods)

## Resolved

- [x] ~~Stack memory leak on task kill~~ — Fixed: stack_frames stored in Task, freed via cleanup_dead_task()
- [x] ~~No task ID recycling~~ — Fixed: freelist-based IdAllocator
- [x] ~~Non-contiguous stack allocation~~ — Fixed: virtual mapping with guard page
- [x] ~~Interrupt frame leak~~ — Fixed: deferred switch model
- [x] ~~Deadlock in panic handler~~ — Fixed: panic_println bypasses lock
- [x] ~~static mut in GDT~~ — Fixed: safe static Stack wrapper
- [x] ~~static mut in APIC~~ — Fixed: AtomicU64
- [x] ~~Kernel memory not protected~~ — Fixed: mark kernel/bootloader regions used
- [x] ~~No OOM handler~~ — Fixed: alloc_error_handler
- [x] ~~Task monopolization~~ — Fixed: fair pick_next_fair
- [x] ~~Interrupts disabled on fresh tasks~~ — Fixed: entry trampoline
- [x] ~~IPC untested~~ — Fixed: integration test (sender→receiver, 18+ messages verified)
- [x] ~~Mailbox not cleaned on death~~ — Fixed: unregister_mailbox in cleanup_dead_task
- [x] ~~check_timeouts deadlock from ISR~~ — Fixed: try_lock in check_timeouts
- [x] ~~PermissionBits::Display heap allocates~~ — Fixed: zero-alloc formatter
- [x] ~~Memory is_subset_of overflow~~ — Fixed: saturating_add
