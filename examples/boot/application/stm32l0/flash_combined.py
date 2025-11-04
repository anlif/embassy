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
        ["cargo", "build", "--release", "--features", "embassy-stm32/stm32c092rc"],
        cwd=BOOTLOADER_DIR
    )

    # Create bootloader binary
    run_command(
        ["cargo", "objcopy", "--release", "--features", "embassy-stm32/stm32c092rc",
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

    # Calculate padding
    gap_size = APP_START_ADDR - FLASH_BASE
    padding_size = gap_size - len(bootloader)

    if padding_size < 0:
        print(f"Error: Bootloader ({len(bootloader)} bytes) is too large!", file=sys.stderr)
        print(f"  Maximum size: {gap_size} bytes ({gap_size/1024:.1f}KB)", file=sys.stderr)
        sys.exit(1)

    # Create combined binary (0xFF is the erased flash value)
    combined = bootloader + (b'\xFF' * padding_size) + app

    combined_path = Path("combined.bin")
    with open(combined_path, 'wb') as f:
        f.write(combined)

    print(f"✓ Combined binary: {len(combined)} bytes ({len(combined)/1024:.1f}KB)")
    print(f"  - Bootloader:  0x{0:08x} - 0x{len(bootloader):08x}")
    print(f"  - Padding:     0x{len(bootloader):08x} - 0x{gap_size:08x} (filled with 0xFF)")
    print(f"  - Application: 0x{gap_size:08x} - 0x{gap_size + len(app):08x}\n")

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
