use core::ptr::write_volatile;
use core::sync::atomic::{Ordering, fence};

use cortex_m::interrupt;

use super::{FlashSector, WRITE_SIZE};
use crate::flash::Error;
use crate::pac;

pub(crate) unsafe fn lock() {
    pac::FLASH.cr().modify(|w| w.set_lock(true));
}
pub(crate) unsafe fn unlock() {
    // Wait, while the memory interface is busy.
    wait_busy();

    // Unlock flash
    if pac::FLASH.cr().read().lock() {
        pac::FLASH.keyr().write_value(0x4567_0123);
        pac::FLASH.keyr().write_value(0xCDEF_89AB);
    }
}

pub(crate) unsafe fn enable_blocking_write() {
    assert_eq!(0, WRITE_SIZE % 4);
    pac::FLASH.cr().write(|w| w.set_pg(true));
}

pub(crate) unsafe fn disable_blocking_write() {
    pac::FLASH.cr().write(|w| w.set_pg(false));
}

pub(crate) unsafe fn blocking_write(start_address: u32, buf: &[u8; WRITE_SIZE]) -> Result<(), Error> {
    let mut address = start_address;
    for val in buf.chunks(4) {
        write_volatile(address as *mut u32, u32::from_le_bytes(unwrap!(val.try_into())));
        address += val.len() as u32;

        // prevents parallelism errors
        fence(Ordering::SeqCst);
    }

    wait_ready_blocking()
}

pub(crate) unsafe fn blocking_erase_sector(sector: &FlashSector) -> Result<(), Error> {
    let idx = (sector.start - super::FLASH_BASE as u32) / super::BANK1_REGION.erase_size as u32;

    #[cfg(feature = "defmt")]
    defmt::trace!("STM32C0 Erase: addr=0x{:08x}, idx={}, erase_size={}", sector.start, idx, super::BANK1_REGION.erase_size);

    // STM32C0 SILICON BUG WORKAROUND:
    // When erasing pages >= 64 with PNB register, pages 0-11 also get erased!
    // This destroys the bootloader region (pages 0-11).
    //
    // ROOT CAUSE: Unknown silicon bug or register mapping error in STM32C092RC
    //
    // WORKAROUND: Only skip erase for pages >= 64 (DFU partition).
    // Pages 0-63 can be erased safely using the normal PNB mechanism.

    // STM32C0 SILICON BUG TESTING: Perform actual erase to observe corruption
    // TODO: Re-enable workaround after testing
    // Normal erase for all pages to test corruption pattern
    wait_busy();
    clear_all_err();

    // Explicitly unlock before erase
    unlock();

    interrupt::free(|_| {
        #[cfg(feature = "defmt")]
        {
            let cr_before = pac::FLASH.cr().read();
            defmt::trace!("FLASH_CR before: 0x{:08x}", cr_before.0);
        }

        // CRITICAL FIX: STM32C0 PAC has wrong PNB bit positions
        // RM0490 specifies PNB is at bits [9:3], but PAC appears to map it incorrectly
        // We need to write PNB directly to the correct bit positions
        pac::FLASH.cr().write(|w| {
            // Manually construct the register value with correct bit positions
            // PER = bit 1, PNB = bits 9:3, STRT = bit 16
            let cr_val = (1 << 1)                      // PER
                       | (((idx as u32) & 0x7F) << 3)  // PNB at bits 9:3
                       | (1 << 16);                    // STRT

            w.0 = cr_val;
            *w
        });

        #[cfg(feature = "defmt")]
        {
            let cr_after = pac::FLASH.cr().read();
            let pnb_manual = (cr_after.0 >> 3) & 0x7F;
            defmt::trace!("FLASH_CR after: 0x{:08x}, PER={}, PNB(auto)={}, PNB(manual)={}, STRT={}",
                         cr_after.0, cr_after.per(), cr_after.pnb(), pnb_manual, cr_after.strt());
        }
    });

    let ret: Result<(), Error> = wait_ready_blocking();

    // Clear erase bit
    pac::FLASH.cr().modify(|w| w.set_per(false));

    // Explicitly lock after erase
    lock();

    // Extra wait to ensure operation completes
    wait_busy();

    ret
}

pub(crate) unsafe fn wait_ready_blocking() -> Result<(), Error> {
    while pac::FLASH.sr().read().bsy() {}

    let sr = pac::FLASH.sr().read();

    if sr.progerr() {
        return Err(Error::Prog);
    }

    if sr.wrperr() {
        return Err(Error::Protected);
    }

    if sr.pgaerr() {
        return Err(Error::Unaligned);
    }

    Ok(())
}

pub(crate) unsafe fn clear_all_err() {
    // read and write back the same value.
    // This clears all "write 1 to clear" bits.
    pac::FLASH.sr().modify(|_| {});
}

#[cfg(any(flash_g0x0, flash_g0x1))]
fn wait_busy() {
    while pac::FLASH.sr().read().bsy() | pac::FLASH.sr().read().bsy2() {}
}

#[cfg(not(any(flash_g0x0, flash_g0x1)))]
fn wait_busy() {
    while pac::FLASH.sr().read().bsy() {}
}

#[cfg(all(bank_setup_configurable, any(flash_g4c2, flash_g4c3, flash_g4c4)))]
pub(crate) fn check_bank_setup() {
    if cfg!(feature = "single-bank") && pac::FLASH.optr().read().dbank() {
        panic!(
            "Embassy is configured as single-bank, but the hardware is running in dual-bank mode. Change the hardware by changing the dbank value in the user option bytes or configure embassy to use dual-bank config"
        );
    }
    if cfg!(feature = "dual-bank") && !pac::FLASH.optr().read().dbank() {
        panic!(
            "Embassy is configured as dual-bank, but the hardware is running in single-bank mode. Change the hardware by changing the dbank value in the user option bytes or configure embassy to use single-bank config"
        );
    }
}

#[cfg(all(bank_setup_configurable, flash_g0x1))]
pub(crate) fn check_bank_setup() {
    if cfg!(feature = "single-bank") && pac::FLASH.optr().read().dual_bank() {
        panic!(
            "Embassy is configured as single-bank, but the hardware is running in dual-bank mode. Change the hardware by changing the dual_bank value in the user option bytes or configure embassy to use dual-bank config"
        );
    }
    if cfg!(feature = "dual-bank") && !pac::FLASH.optr().read().dual_bank() {
        panic!(
            "Embassy is configured as dual-bank, but the hardware is running in single-bank mode. Change the hardware by changing the dual_bank value in the user option bytes or configure embassy to use single-bank config"
        );
    }
}
