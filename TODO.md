# Technical Debt & Known Issues

## Critical

- [ ] **Cooperative preemption only** — tasks must call `should_reschedule()` + `yield_now()`. True preemption requires interrupt-return patching.
- [ ] **Timer interval not calibrated** — `0x20000` is arbitrary. Should calibrate against PIT or TSC.

## Important

- [ ] **No ring-3 user space** — syscall MSRs are set up but no user-mode page tables or ELF loading into separate address space. All code runs in ring-0.
- [ ] **OxideFS not persisted to disk** — still in-memory only. Need to write superblock + blobs to virtio-blk blocks.
- [ ] **No SMP support** — single-core only.
- [ ] **Frame allocator is O(n)** — linear bitmap scan.
- [ ] **No unmap/free for page tables** — dead task stacks stay mapped.
- [ ] **Heap cannot grow** — fixed 1 MiB.
- [ ] **virtio-blk DMA addresses** — using virtual addresses for descriptors, works in QEMU emulation but not on real hardware.
- [ ] **No TLS** — HTTP only, not HTTPS. Need TLS 1.3 for real API calls.

## Nice to Have

- [ ] Time-slice accounting
- [ ] Priority inheritance
- [ ] Serial input for interactive debugging
- [ ] Structured kernel logging with levels
- [ ] DHCP (currently static IP)
- [ ] ARP resolution (currently relies on QEMU user-mode networking)

## Resolved

- [x] ~~virtio-net skeleton~~ — Fixed: real PCI driver with split virtqueues (v1.1.0)
- [x] ~~virtio-blk stub~~ — Fixed: real PCI driver with virtqueue block I/O (v1.1.0)
- [x] ~~HTTP returns placeholder~~ — Fixed: real TCP socket flow with timeouts (v1.1.0)
- [x] ~~signing.verify() always true~~ — Fixed: real HMAC constant-time comparison (v1.1.0)
- [x] ~~Syscalls not wired~~ — Fixed: EFER.SCE + LSTAR + STAR + SFMASK + naked entry (v1.1.0)
- [x] ~~Stack memory leak on task kill~~ — Fixed: cleanup_dead_task (v0.5.0)
- [x] ~~No task ID recycling~~ — Fixed: freelist IdAllocator (v0.5.0)
- [x] ~~Non-contiguous stack allocation~~ — Fixed: virtual mapping (v0.2.0)
- [x] ~~Interrupt frame leak~~ — Fixed: deferred switch (v0.2.0)
- [x] ~~Deadlock in panic handler~~ — Fixed: panic_println (v0.1.0)
- [x] ~~static mut in GDT/APIC~~ — Fixed: safe wrappers (v0.2.0)
- [x] ~~Kernel memory not protected~~ — Fixed: mark regions used (v0.1.0)
- [x] ~~Task monopolization~~ — Fixed: fair pick_next_fair (v0.2.0)
- [x] ~~IPC untested~~ — Fixed: integration test (v0.5.0)
- [x] ~~check_timeouts deadlock~~ — Fixed: try_lock (v0.4.0)
- [x] ~~PermissionBits::Display heap alloc~~ — Fixed: zero-alloc formatter (v0.3.0)
- [x] ~~O(n*m) dedup~~ — Fixed: FNV-1a hash (v0.7.0)
- [x] ~~Lock ordering undocumented~~ — Fixed: comments added (v0.7.0)
- [x] ~~Socket lock ordering deadlock~~ — Fixed: INTERFACE→SOCKETS (v0.6.0)
- [x] ~~128 KiB per socket~~ — Fixed: 8 KiB buffers (v0.6.0)
- [x] ~~HTTP ignoring capabilities~~ — Fixed: validate + firewall (v0.6.0)
