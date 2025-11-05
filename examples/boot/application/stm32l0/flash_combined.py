#!/usr/bin/env python3
"""
Flash combined bootloader + application binary for STM32C092RC
This works around a probe-rs flash corruption bug on STM32C0 series
"""

import subprocess
import sys
from pathlib import Path

# Configuration
BOOTLOADER_DIR = Path("../../bootloader/stm32")
CHIP = "STM32C092RCTx"
APP_START_ADDR = 0x08007000  # Application starts here in flash
FLASH_BASE = 0x08000000


def run_command(cmd, cwd=None, description=None):
    """Run a command and handle errors"""
    if description:
        print(f"{description}...")

    try:
        result = subprocess.run(
            cmd,
            cwd=cwd,
            check=True,
            capture_output=False,  # Show output in real-time
            text=True
        )
        return result
    except subprocess.CalledProcessError as e:
        print(f"\nError: Command failed with exit code {e.returncode}", file=sys.stderr)
        sys.exit(1)


def build_bootloader():
    """Build the bootloader"""
    print("Building bootloader...")

    # Build bootloader ELF
    run_command(
        ["cargo", "build", "--release", "--features", "defmt"],
        cwd=BOOTLOADER_DIR
    )

    # Create bootloader binary
    run_command(
        ["cargo", "objcopy", "--release", "--features", "defmt",
         "--", "-O", "binary", "bootloader.bin"],
        cwd=BOOTLOADER_DIR
    )

    bootloader_bin = BOOTLOADER_DIR / "bootloader.bin"
    size = bootloader_bin.stat().st_size
    print(f"✓ Bootloader binary: {size} bytes ({size/1024:.1f}KB)\n")
    return bootloader_bin


def build_application():
    """Build the application"""
    print("Building application...")

    # Build application ELF
    run_command(
        ["cargo", "build", "--release", "--bin", "a", "--features", "defmt"]
    )

    # Create application binary
    run_command(
        ["cargo", "objcopy", "--release", "--bin", "a", "--features", "defmt",
         "--", "-O", "binary", "app.bin"]
    )

    app_bin = Path("app.bin")
    size = app_bin.stat().st_size
    print(f"✓ Application binary: {size} bytes ({size/1024:.1f}KB)\n")
    return app_bin


def create_combined_binary(bootloader_bin, app_bin):
    """Create combined binary with padding"""
    print("Creating combined binary...")

    # Read binaries
    with open(bootloader_bin, 'rb') as f:
        bootloader = f.read()

    with open(app_bin, 'rb') as f:
        app = f.read()

    # Memory layout:
    # 0x08000000 - 0x08006000: Bootloader (24K)
    # 0x08006000 - 0x08007000: Bootloader State (4K)
    # 0x08007000 - 0x08025000: Active partition (120K) - APP A goes here
    # 0x08025000 - 0x08043800: DFU partition (122K)

    BOOTLOADER_END = 0x08006000
    STATE_END = 0x08007000
    ACTIVE_START = 0x08007000

    bootloader_size = len(bootloader)
    bootloader_max = BOOTLOADER_END - FLASH_BASE

    if bootloader_size > bootloader_max:
        print(f"Error: Bootloader ({bootloader_size} bytes) is too large!", file=sys.stderr)
        print(f"  Maximum size: {bootloader_max} bytes ({bootloader_max/1024:.1f}KB)", file=sys.stderr)
        sys.exit(1)

    # Calculate padding needed
    padding_to_state = (BOOTLOADER_END - FLASH_BASE) - bootloader_size
    state_padding = STATE_END - BOOTLOADER_END  # 4K state partition

    # Create combined binary (0xFF is the erased flash value)
    combined = (
        bootloader +
        (b'\xFF' * padding_to_state) +  # Pad to state partition
        (b'\xFF' * state_padding) +      # State partition (erased)
        app                               # Application in ACTIVE partition
    )

    combined_path = Path("combined.bin")
    with open(combined_path, 'wb') as f:
        f.write(combined)

    print(f"✓ Combined binary: {len(combined)} bytes ({len(combined)/1024:.1f}KB)")
    print(f"  - Bootloader:      0x{FLASH_BASE:08x} - 0x{FLASH_BASE + bootloader_size:08x} ({bootloader_size} bytes)")
    print(f"  - Padding:         0x{FLASH_BASE + bootloader_size:08x} - 0x{BOOTLOADER_END:08x}")
    print(f"  - State (erased):  0x{BOOTLOADER_END:08x} - 0x{STATE_END:08x}")
    print(f"  - Application:     0x{ACTIVE_START:08x} - 0x{ACTIVE_START + len(app):08x} ({len(app)} bytes)\n")

    return combined_path


def flash_binary(binary_path):
    """Flash the combined binary using probe-rs"""
    print(f"Flashing {binary_path.name}...")

    run_command(
        ["probe-rs", "download",
         "--chip", CHIP,
         "--binary-format", "bin",
         "--base-address", f"0x{FLASH_BASE:08x}",
         str(binary_path)],
        description="Flashing"
    )

    print("✓ Flash complete\n")


def verify_binaries():
    """Verify both bootloader and application"""
    print("Verifying flash contents...")

    # Verify bootloader
    bootloader_elf = BOOTLOADER_DIR / "target/thumbv6m-none-eabi/release/stm32-bootloader-example"
    print("  Verifying bootloader...")
    run_command(
        ["probe-rs", "verify", "--chip", CHIP, str(bootloader_elf)]
    )
    print("  ✓ Bootloader verified")

    # Verify application
    app_elf = Path("target/thumbv6m-none-eabi/release/a")
    print("  Verifying application...")
    run_command(
        ["probe-rs", "verify", "--chip", CHIP, str(app_elf)]
    )
    print("  ✓ Application verified\n")


def main():
    """Main script execution"""
    print("=" * 60)
    print("STM32C092RC Combined Bootloader + Application Flash Tool")
    print("=" * 60)
    print()

    try:
        # Build binaries
        bootloader_bin = build_bootloader()
        app_bin = build_application()

        # Create combined binary
        combined_bin = create_combined_binary(bootloader_bin, app_bin)

        # Flash
        flash_binary(combined_bin)

        # Verify
        verify_binaries()

        print("=" * 60)
        print("✅ SUCCESS: Flash and verification complete!")
        print("=" * 60)

    except KeyboardInterrupt:
        print("\n\n⚠️  Interrupted by user", file=sys.stderr)
        sys.exit(130)
    except Exception as e:
        print(f"\n\n❌ Error: {e}", file=sys.stderr)
        import traceback
        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    main()
