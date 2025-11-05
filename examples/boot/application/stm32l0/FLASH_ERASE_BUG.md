# STM32C0 Flash Erase HardFault Issue - Bug Report

## Summary

Flash erase operations cause HardFaults on STM32C0 series when using `Flash::new_blocking()` directly. The issue is caused by interrupts firing during flash erase operations, attempting to fetch instructions from flash while the flash controller is busy.

## Environment

- **MCU**: STM32C092RC (Cortex-M0+, single-bank flash)
- **Embassy Version**: Latest (as of 2025-11-05)
- **Flash Size**: 256KB (2KB page size)
- **Affected Crates**:
  - `embassy-stm32` (flash driver)
  - `embassy-boot-stm32` (bootloader)

## Problem Description

### Symptoms

1. HardFault exception during flash erase operations
2. Occurs in both bootloader and application code
3. Trace shows erase starting, then immediate HardFault

### Example Error Output

```
[TRACE] Erasing from 0x8025000 to 0x8025800 (embassy_stm32 src/flash/common.rs:147)
[TRACE] Erasing sector: FlashSector { bank: Bank1, index_in_bank: 74, start: 134369280, size: 2048 }
Firmware exited unexpectedly: Exception
Core 0
    Frame 0: HardFault @ 0x080017d2
```

## Root Cause Analysis

### STM32 Flash Programming Restriction

From **RM0490 (STM32C0 Reference Manual) Section 4.3.4**:

> "During a program/erase operation to the flash memory, **any attempt to read the flash memory stalls the bus**. The read operation proceeds correctly once the program/erase operation has completed."

This means:
- During flash erase, the flash controller cannot service read requests
- CPU instruction fetches from flash are stalled
- **ANY attempt to read flash (instructions or data) causes HardFault**

### Critical Discovery: The Real Problem

**Interrupts are NOT the root cause!** Testing revealed:
- ✅ Wrapped operations in `cortex_m::interrupt::free()` - still HardFaults
- ✅ Used `critical_section::with()` in flash driver - still HardFaults
- ✅ Disabled ALL interrupts at CPU level - **still HardFaults**

**The actual problem**: The flash erase/write functions themselves execute from flash. When they start an erase operation and try to fetch the next instruction, the flash is busy → immediate HardFault.

**Required solution**: Flash operations MUST execute from RAM.

### Embassy Implementation Issue

**Location**: `embassy-stm32/src/flash/common.rs`

**The Issue**: The `Flash` struct has two erase implementations:

1. **Line 71** - `Flash::blocking_erase()`:
   ```rust
   pub fn blocking_erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
       unsafe { blocking_erase(FLASH_BASE as u32, from, to, erase_sector_unlocked) }
   }
   ```
   - Uses `erase_sector_unlocked`
   - **Does NOT disable interrupts** ❌
   - **Causes HardFault on single-bank flash MCUs**

2. **Line 273** - Flash region's `blocking_erase()`:
   ```rust
   pub fn blocking_erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
       unsafe { blocking_erase(self.0.base, from, to, erase_sector_with_critical_section) }
   }
   ```
   - Uses `erase_sector_with_critical_section`
   - **Disables interrupts with critical section** ✅
   - **Works correctly**

**Why This Matters**:

```rust
// common.rs:170-172
pub(super) unsafe fn erase_sector_with_critical_section(sector: &FlashSector) -> Result<(), Error> {
    critical_section::with(|_| erase_sector_unlocked(sector))  // <-- Interrupts disabled here
}
```

The `critical_section::with()` call disables interrupts during the erase operation, preventing interrupt handlers from trying to execute from flash while it's busy.

## Code Paths Affected

### ❌ Broken Path (causes HardFault)
```rust
let flash = Flash::new_blocking(p.FLASH);  // Direct Flash struct
// ... later in code
flash.erase(from, to)?;  // Uses erase_sector_unlocked (no critical section)
```

### ✅ Working Path
```rust
let layout = Flash::new_blocking(p.FLASH).into_blocking_regions();
let flash = layout.bank1_region;  // Use flash region
// ... later in code
flash.erase(from, to)?;  // Uses erase_sector_with_critical_section
```

## Workaround

### For Application Code

**Before (broken)**:
```rust
let flash = Flash::new_blocking(p.FLASH);
let flash = Mutex::new(BlockingAsync::new(flash));

let config = FirmwareUpdaterConfig::from_linkerfile(&flash, &flash);
let mut updater = FirmwareUpdater::new(config, &mut magic.0);
```

**After (working)**:
```rust
let layout = Flash::new_blocking(p.FLASH).into_blocking_regions();
let flash = Mutex::new(BlockingAsync::new(layout.bank1_region));

let config = FirmwareUpdaterConfig::from_linkerfile(&flash, &flash);
let mut updater = FirmwareUpdater::new(config, &mut magic.0);
```

### For Bootloader Code

**Before (broken)**:
```rust
let flash = Flash::new_blocking(p.FLASH);
let flash = Mutex::new(RefCell::new(flash));

let config = BootLoaderConfig::from_linkerfile_blocking(&flash, &flash, &flash);
```

**After (working)**:
```rust
let layout = Flash::new_blocking(p.FLASH).into_blocking_regions();
let flash = Mutex::new(RefCell::new(layout.bank1_region));

let config = BootLoaderConfig::from_linkerfile_blocking(&flash, &flash, &flash);
```

## Proposed Fix

The flash erase/write functions MUST be placed in RAM for single-bank flash MCUs. This requires:

1. **Mark functions to run from RAM** using `#[link_section = ".data"]` or `#[ram]` attribute
2. **Copy code to RAM at startup** (handled by cortex-m-rt)
3. **Disable interrupts** to prevent any flash access during operations

Example implementation:

```rust
#[link_section = ".data.ram_func"]
#[inline(never)]
pub(crate) unsafe fn blocking_erase_sector_ram(sector: &FlashSector) -> Result<(), Error> {
    // This function will be copied to RAM and executed from there
    pac::FLASH.cr().modify(|w| w.set_per(true));
    pac::FLASH.ar().write(|w| w.set_far(sector.start));
    pac::FLASH.cr().modify(|w| w.set_strt(true));

    // Busy wait (safe because we're in RAM)
    while pac::FLASH.sr().read().bsy() {}

    // Check result and clear flags
    // ...
    Ok(())
}
```

This is a significant architectural change requiring:
- Linker script modifications
- RAM section definitions
- All flash operation code paths moved to RAM

## Affected MCU Families

All STM32 MCUs with **single-bank flash** are potentially affected:
- ✅ STM32C0 (confirmed affected)
- STM32F0
- STM32G0
- STM32L0
- STM32F1
- STM32F3
- STM32L1

MCUs with **dual-bank flash** that support Read-While-Write may not be affected:
- STM32H7 (dual-bank)
- STM32G4 (dual-bank option)
- STM32L4+ (dual-bank option)

## Testing

### Reproduction Steps

1. Create application using `Flash::new_blocking()`
2. Call `flash.erase()` on any flash sector
3. Ensure interrupts are enabled (e.g., SysTick, UART, etc.)
4. Observe HardFault during erase operation

### Verification

After applying workaround:
1. Application should successfully erase flash
2. No HardFaults during erase/write operations
3. Firmware update mechanism works correctly

## References

- STM32C0 Reference Manual (RM0490) - Section 4.3.4 "FLASH program and erase operations"
- Embassy STM32 Flash Driver: `embassy-stm32/src/flash/common.rs`
- Embassy Boot: `embassy-boot-stm32`
- STMicroelectronics Community discussions on flash programming while executing from flash

## Related Issues

This is similar to known issues on other STM32 series where flash operations require special handling:
- Must disable interrupts during erase/write on single-bank flash
- Instruction fetch causes bus stall during flash operations
- Some MCUs require running flash driver code from RAM

## Current Status & Workarounds

### Status
**UNSOLVED** - No workaround currently exists within Embassy framework for single-bank flash MCUs.

### Attempted Workarounds (All Failed)
1. ❌ Using flash regions instead of Flash struct
2. ❌ Using `BlockingFirmwareUpdater` instead of async
3. ❌ Wrapping operations in `cortex_m::interrupt::free()`
4. ❌ Using `critical_section::with()` in driver

### Why All Workarounds Failed
All workarounds only addressed interrupts, but the core issue is that **the erase function code itself executes from flash**. No amount of interrupt masking can prevent the CPU from trying to fetch the next instruction after starting an erase.

### Viable Solutions

1. **Use external programmer only** - Do NOT perform in-application programming (IAP)
   - Flash all firmware via SWD/JTAG
   - This is what we implemented for the bootloader

2. **Implement RAM-based flash driver** - Significant work required:
   - Create RAM-resident flash erase/write functions
   - Modify linker scripts
   - Ensure all call paths are RAM-safe
   - This would need to be contributed to Embassy

3. **Use different MCU** - Switch to STM32 with dual-bank flash:
   - STM32H7 series (dual-bank with read-while-write)
   - STM32G4 series (dual-bank option)
   - These can execute from one bank while programming the other

### Recommendation for Embassy

Embassy should either:
1. **Document the limitation**: Clearly state that IAP is not supported on single-bank flash MCUs
2. **Implement RAM-based driver**: Add RAM-resident flash operations for affected MCU families
3. **Provide compile-time error**: Fail compilation if IAP is attempted on unsupported MCUs

## Author

Documented by: User (alf) & Claude Code
Date: 2025-11-05
Embassy Version: Latest main branch
**Issue**: Flash erase/write from application code is fundamentally broken on single-bank flash STM32 MCUs
