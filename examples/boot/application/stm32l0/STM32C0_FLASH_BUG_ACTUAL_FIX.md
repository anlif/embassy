# STM32C0 Flash Driver Bug - ACTUAL ROOT CAUSE AND FIX

## Summary

STM32C0 flash erase/write operations were causing immediate panics because **the flash driver was not configured** in Embassy. The STM32C0 was falling through to the "other.rs" stub driver which has all functions as `unimplemented!()`.

## The Actual Bug

**Location**: `embassy-stm32/src/flash/mod.rs` lines 101-115

### Problem

The module path selection did not include `flash_c0`:

```rust
#[cfg_attr(any(flash_g0x0, flash_g0x1, flash_g4c2, flash_g4c3, flash_g4c4), path = "g.rs")]
// ... other flash families ...
#[cfg_attr(
    not(any(
        flash_l0, flash_l1, /* ... */ flash_g4c4, flash_h7, /* ... */
        // flash_c0 was MISSING from this list!
    )),
    path = "other.rs"  // ← STM32C0 fell through to here
)]
```

### What Was Happening

1. STM32C092RC has cfg flag `flash_c0` enabled during compilation
2. The cfg_attr list did not include `flash_c0` in any of the specific drivers
3. STM32C0 fell through to the catch-all `other.rs`
4. `other.rs` contains only stub implementations:
   ```rust
   pub(crate) unsafe fn unlock() {
       unimplemented!();
   }
   pub(crate) unsafe fn blocking_erase_sector(_sector: &FlashSector) -> Result<(), Error> {
       unimplemented!();
   }
   ```
5. When application called flash erase → `unimplemented!()` → **panic** → HardFault

### Why We Thought It Was Something Else

The panic happened so early and manifested as HardFault at address `0x080000c0`, which appeared to be a flash access issue. The trace showed:

```
[TRACE] Erasing sector: FlashSector { ... }
Firmware exited unexpectedly: Exception
```

This made it look like the erase operation itself was failing due to bus stalling or execution from flash. In reality, the erase was never even attempted - it panicked on `unimplemented!()`.

## The Fix

**File**: `embassy-stm32/src/flash/mod.rs`

### Change 1: Add flash_c0 to g.rs driver mapping (line 101)

```rust
#[cfg_attr(any(flash_g0x0, flash_g0x1, flash_g4c2, flash_g4c3, flash_g4c4, flash_c0), path = "g.rs")]
```

STM32C0 is closely related to STM32G0 and uses the same flash controller, so it should use `g.rs`.

### Change 2: Add flash_c0 to the exclusion list (line 111)

```rust
#[cfg_attr(
    not(any(
        flash_l0, flash_l1, flash_l4, flash_l5, flash_wl, flash_wb, flash_f0, flash_f1, flash_f2, flash_f3, flash_f4,
        flash_f7, flash_g0x0, flash_g0x1, flash_g4c2, flash_g4c3, flash_g4c4, flash_c0, flash_h7, flash_h7ab, flash_u5,
        flash_wba, flash_h50, flash_u0, flash_h5,
    )),
    path = "other.rs"
)]
```

This ensures flash_c0 doesn't fall through to `other.rs`.

## Why The G Driver Works

The `g.rs` driver (used by G0/G4) has proper STM32 flash support:

1. **Unlock with busy wait**:
   ```rust
   pub(crate) unsafe fn unlock() {
       wait_busy();  // Wait for any ongoing operations
       if pac::FLASH.cr().read().lock() {
           pac::FLASH.keyr().write_value(0x4567_0123);
           pac::FLASH.keyr().write_value(0xCDEF_89AB);
       }
   }
   ```

2. **Erase with interrupt protection**:
   ```rust
   pub(crate) unsafe fn blocking_erase_sector(sector: &FlashSector) -> Result<(), Error> {
       wait_busy();
       clear_all_err();

       interrupt::free(|_| {  // ← Disables interrupts during start
           pac::FLASH.cr().modify(|w| {
               w.set_per(true);
               w.set_pnb(idx as u8);
               w.set_strt(true);
           });
       });

       wait_ready_blocking()  // Wait for completion with proper error checking
   }
   ```

3. **Proper error handling**:
   ```rust
   pub(crate) unsafe fn wait_ready_blocking() -> Result<(), Error> {
       while pac::FLASH.sr().read().bsy() {}

       let sr = pac::FLASH.sr().read();
       if sr.progerr() { return Err(Error::Prog); }
       if sr.wrperr() { return Err(Error::Protected); }
       if sr.pgaerr() { return Err(Error::Unaligned); }
       Ok(())
   }
   ```

## Test Results

### Before Fix
```
[TRACE] Erasing sector: FlashSector { bank: Bank1, index_in_bank: 74, ... }
Firmware exited unexpectedly: Exception
Frame 0: HardFault @ 0x080000c0
```

Immediate panic on first erase attempt.

### After Fix
```
[INFO ] Writing chunk 0 at offset 0x0
[TRACE] Erasing from 0x8025000 to 0x8025800
[TRACE] Erasing sector: FlashSector { bank: Bank1, index_in_bank: 74, ... }
[TRACE] Writing 8 bytes at 0x8025000
[INFO ] Writing chunk 1 at offset 0x8
[TRACE] Writing 8 bytes at 0x8025008
...
[INFO ] Writing chunk 1024 at offset 0x2000
[TRACE] Erasing from 0x8027000 to 0x8027800
```

Successfully erased first sector, wrote 1024 chunks (8KB), started second sector erase.

## Related Issues

### The Bus Stall Investigation

During debugging, we investigated whether the CPU stalling during flash erase (as documented in RM0490) was causing HardFaults. This is a real consideration for single-bank flash MCUs, but it turned out to be a red herring.

**Key finding**: The RM0490 states the bus **stalls** during flash operations, which is normal behavior. The CPU waits for the operation to complete. It does NOT cause HardFaults under normal circumstances.

### Why Interrupts Still Matter

While not the root cause of THIS bug, the `interrupt::free()` in `g.rs` is still important:

- Prevents interrupts from firing during the critical "start erase" operation
- Ensures atomic register modifications
- Reduces race conditions with interrupt handlers

## Affected Chips

All STM32C0 series chips:
- STM32C011
- STM32C031
- STM32C071
- STM32C091
- **STM32C092** (our test chip)

## Fix Status

**Status**: FIXED in local Embassy fork

**Required for upstream**:
1. Submit PR to Embassy GitHub with the two-line fix in `mod.rs`
2. Add test coverage for STM32C0 flash operations
3. Consider adding compile-time error if a flash family falls through to `other.rs`

## Lessons Learned

1. **Check cfg flags first**: When operations fail immediately, verify the correct driver is being used
2. **other.rs is a trap**: Falling through to stub implementations causes confusing errors
3. **Follow the types**: The `unimplemented!()` panic should have been more obvious in hindsight
4. **Documentation matters**: RM0490 bus stall behavior is correct but not the issue here

## Author

Discovered and fixed by: User (alf) & Claude Code
Date: 2025-11-05
Embassy Version: Latest main branch
**Issue**: STM32C0 missing from flash driver cfg_attr list
**Solution**: Add `flash_c0` to g.rs driver mapping
