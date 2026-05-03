# Real I/O Implementation Plan — Making Everything Actually Work

> Turn all placeholder/skeleton code into real, functional implementations.

**Goal:** After this plan, Oxide OS has real packet flow (agents can make TCP connections), real disk persistence (data survives reboot), real syscall instruction handling (ring-3 isolation), and real cryptographic signing.

---

## Priority Order

| # | Task | Impact | Depends On |
|---|------|--------|------------|
| 1 | Wire virtio-net into smoltcp | Enables real TCP/IP | — |
| 2 | Real HTTP over TCP | Agents can call LLM APIs | Task 1 |
| 3 | Real virtio-blk driver | Enables disk I/O | — |
| 4 | OxideFS disk persistence | Data survives reboot | Task 3 |
| 5 | Syscall instruction wiring | Real ring-3 isolation | — |
| 6 | Fix agent signing | Real Ed25519 verification | — |

---

## Task 1: Wire virtio-net RX/TX into smoltcp Device trait

**Files:** Modify `net/stack.rs`

The `OxideNetDevice` currently has a dummy `receive()` that always returns `None`. Wire it to the real `VirtioNetDevice::receive()` and `transmit()`.

**Key change:** The smoltcp `RxToken::consume` and `TxToken::consume` must use the real virtqueue buffers instead of allocating dummy vecs.

---

## Task 2: Real HTTP over TCP

**Files:** Modify `net/http.rs`

Replace placeholder responses with actual TCP socket flow:
1. Resolve hostname via DNS cache
2. `tcp_create()` + `tcp_connect()` to target
3. Poll until connected
4. Send HTTP request bytes
5. Poll + receive response bytes
6. Parse HTTP response (status line + headers + body)
7. Close socket

---

## Task 3: Real virtio-blk driver

**Files:** Rewrite `storage/virtio_blk.rs`, add PCI discovery

Same pattern as virtio-net:
1. PCI scan for vendor `0x1AF4`, device `0x1001` (block device)
2. BAR0 I/O space setup
3. Virtqueue for block requests
4. Read/write block commands via virtqueue descriptors

---

## Task 4: OxideFS disk persistence

**Files:** Modify `storage/oxide_fs.rs`, `storage/block_cache.rs`

Wire OxideFS to use the block cache (which uses virtio-blk):
1. Superblock at block 0 (magic, version, blob count, index offset)
2. Write blobs to sequential blocks
3. Write index entries to blocks
4. On init, read superblock and reconstruct in-memory index

---

## Task 5: Syscall instruction wiring

**Files:** Modify `syscall/mod.rs`, create assembly entry

Wire the x86_64 `syscall` instruction:
1. Set EFER.SCE (System Call Extensions)
2. Set LSTAR to syscall entry point
3. Set STAR with kernel/user CS/SS selectors
4. Set SFMASK to mask interrupts on entry
5. Naked assembly handler saves registers, calls `dispatch()`, does `sysretq`

---

## Task 6: Fix agent signing

**Files:** Modify `crypto/signing.rs`

Replace placeholder `verify()` that always returns true with real HMAC-based verification:
1. `sign()` produces HMAC-SHA256(secret_key || message)
2. `verify()` recomputes and compares (constant-time)
