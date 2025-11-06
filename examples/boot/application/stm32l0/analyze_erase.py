#!/usr/bin/env python3
"""
Analyze which pages were erased
"""
import subprocess

CHIP = "STM32C092RCTx"
PAGE_SIZE = 2048
FLASH_BASE = 0x08000000

def read_page(addr):
    """Read a full page and check if it's erased"""
    try:
        result = subprocess.run(
            ["probe-rs", "read", "b32", hex(addr), str(PAGE_SIZE // 4), "--chip", CHIP],
            capture_output=True,
            text=True,
            check=True
        )
        # Check if all values are 0xffffffff (erased)
        values = result.stdout.strip().split()
        is_erased = all(v == 'ffffffff' for v in values)
        return is_erased
    except:
        return None

def main():
    print("Scanning flash to find erased pages...")
    print("=" * 60)

    # Check first 32 pages (64KB)
    for page_num in range(32):
        addr = FLASH_BASE + (page_num * PAGE_SIZE)
        is_erased = read_page(addr)

        # Determine region
        if addr < 0x08006000:
            region = "BOOTLOADER"
        elif addr < 0x08007000:
            region = "STATE"
        elif addr < 0x08020000:
            region = "ACTIVE"
        else:
            region = "DFU"

        status = "ERASED" if is_erased else "HAS DATA"
        marker = "⚠️" if (region == "BOOTLOADER" and is_erased) else ""

        print(f"Page {page_num:2d} (0x{addr:08x}): {status:10s} [{region:12s}] {marker}")

if __name__ == "__main__":
    main()
