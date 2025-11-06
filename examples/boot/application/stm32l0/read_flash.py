#!/usr/bin/env python3
"""
Read and display flash memory contents from STM32C092RC
"""
import subprocess
import sys

CHIP = "STM32C092RCTx"

def read_memory(address, size):
    """Read memory from target using probe-rs"""
    try:
        result = subprocess.run(
            ["probe-rs", "read", "b32", hex(address), str(size), "--chip", CHIP],
            capture_output=True,
            text=True,
            check=True
        )
        return result.stdout
    except subprocess.CalledProcessError as e:
        print(f"Error reading memory: {e.stderr}", file=sys.stderr)
        return None

def main():
    print("Reading flash memory contents...")
    print("=" * 60)

    # Read bootloader vector table
    print("\n📍 Bootloader Vector Table (0x08000000):")
    print(read_memory(0x08000000, 8))

    # Read ACTIVE partition vector table
    print("\n📍 ACTIVE Partition Vector Table (0x08007000):")
    print(read_memory(0x08007000, 8))

    # Read state partition
    print("\n📍 BOOTLOADER_STATE Partition (0x08006000):")
    print(read_memory(0x08006000, 8))

    print("\n" + "=" * 60)

if __name__ == "__main__":
    main()
