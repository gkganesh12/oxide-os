# Oxide OS Makefile

KERNEL := target/x86_64-unknown-none/debug/oxide-kernel
ISO := oxide-os.iso
LIMINE_DIR := limine

.PHONY: all kernel iso run clean

all: iso

# Build the kernel
kernel:
	cargo build

# Clone and build Limine bootloader (v8.x binary branch)
$(LIMINE_DIR):
	git clone https://github.com/limine-bootloader/limine.git --branch=v8.x-binary --depth=1 $(LIMINE_DIR)
	$(MAKE) -C $(LIMINE_DIR)

# Build the bootable ISO
iso: kernel $(LIMINE_DIR)
	rm -rf iso_root
	mkdir -p iso_root/boot
	cp $(KERNEL) iso_root/boot/kernel
	cp limine.conf iso_root/
	mkdir -p iso_root/boot/limine
	cp $(LIMINE_DIR)/limine-bios-cd.bin iso_root/boot/limine/
	cp $(LIMINE_DIR)/limine-bios.sys iso_root/boot/limine/
	@if [ -f $(LIMINE_DIR)/limine-uefi-cd.bin ]; then \
		cp $(LIMINE_DIR)/limine-uefi-cd.bin iso_root/boot/limine/; \
	fi
	xorriso -as mkisofs \
		-b boot/limine/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		iso_root -o $(ISO)
	./$(LIMINE_DIR)/limine bios-install $(ISO)

# Run in QEMU
run: iso
	qemu-system-x86_64 -cdrom $(ISO) -serial stdio -no-reboot -no-shutdown

# Clean build artifacts
clean:
	cd kernel && cargo clean
	rm -rf iso_root $(ISO)
