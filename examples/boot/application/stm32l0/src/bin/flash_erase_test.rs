#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::flash::{Flash, Blocking};
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::Timer;
use panic_reset as _;

const PAGE_SIZE: usize = 2048;
const TOTAL_PAGES: usize = 128; // STM32C092RC has 128 pages (256KB / 2KB)
const FLASH_BASE: u32 = 0x08000000;

// Test pages in this range (DFU partition)
// DFU at 0x08020000 = page 64, length 102K = 51 pages (64-114)
const TEST_START_PAGE: usize = 64;  // Start from DFU area
const TEST_END_PAGE: usize = 115;   // Test all DFU pages

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    let mut led = Output::new(p.PC9, Level::Low, Speed::Low);
    let mut flash = Flash::new_blocking(p.FLASH);

    info!("STM32C0 Flash Erase Corruption Test");
    info!("====================================");
    info!("Flash: {} pages of {} bytes each", TOTAL_PAGES, PAGE_SIZE);
    info!("Testing pages {} to {}", TEST_START_PAGE, TEST_END_PAGE - 1);
    info!("");

    // Blink to indicate test starting
    for _ in 0..3 {
        led.set_high();
        Timer::after_millis(200).await;
        led.set_low();
        Timer::after_millis(200).await;
    }

    Timer::after_secs(2).await;

    // Test each page
    for page_num in TEST_START_PAGE..TEST_END_PAGE {
        test_page_erase(&mut flash, &mut led, page_num).await;

        // Small delay between tests
        Timer::after_millis(100).await;
    }

    info!("");
    info!("=== TEST COMPLETE ===");

    // Continuous slow blink to indicate done
    loop {
        led.set_high();
        Timer::after_secs(1).await;
        led.set_low();
        Timer::after_secs(1).await;
    }
}

async fn test_page_erase(flash: &mut Flash<'_, Blocking>, led: &mut Output<'_>, page_num: usize) {
    info!("");
    info!("Testing page {}", page_num);

    // Calculate addresses
    let page_offset = (page_num * PAGE_SIZE) as u32;
    let page_addr = FLASH_BASE + page_offset;

    info!("  Address: 0x{:08x} (offset 0x{:x})", page_addr, page_offset);

    // Check sentinel regions BEFORE erase
    let bootloader_ok_before = check_bootloader_intact();
    let flash_base_ok_before = check_flash_base();

    info!("  Before: Bootloader={}, FLASH_BASE={}",
          if bootloader_ok_before { "OK" } else { "CORRUPT" },
          if flash_base_ok_before { "OK" } else { "CORRUPT" });

    // Perform the erase
    led.set_high();

    let erase_start = page_offset;
    let erase_end = page_offset + PAGE_SIZE as u32;

    match flash.blocking_erase(erase_start, erase_end) {
        Ok(_) => {
            info!("  Erase: SUCCESS");
        }
        Err(_) => {
            error!("  Erase: FAILED");
            led.set_low();
            return;
        }
    }

    led.set_low();

    // Check sentinel regions AFTER erase
    let bootloader_ok_after = check_bootloader_intact();
    let flash_base_ok_after = check_flash_base();

    info!("  After:  Bootloader={}, FLASH_BASE={}",
          if bootloader_ok_after { "OK" } else { "CORRUPT" },
          if flash_base_ok_after { "OK" } else { "CORRUPT" });

    // Detect corruption
    if !bootloader_ok_after && bootloader_ok_before {
        error!("  >>> BOOTLOADER CORRUPTED BY PAGE {} ERASE <<<", page_num);

        // Rapid blink to indicate corruption found
        for _ in 0..10 {
            led.set_high();
            Timer::after_millis(50).await;
            led.set_low();
            Timer::after_millis(50).await;
        }
    }

    if !flash_base_ok_after && flash_base_ok_before {
        error!("  >>> FLASH_BASE CORRUPTED BY PAGE {} ERASE <<<", page_num);
    }

    // Check if the page itself was erased correctly
    if check_page_erased(page_addr) {
        info!("  Verify: Page correctly erased to 0xFF");
    } else {
        warn!("  Verify: Page NOT fully erased!");
    }
}

fn check_bootloader_intact() -> bool {
    // Check bootloader vector table at 0x08000000
    unsafe {
        let stack_ptr = core::ptr::read_volatile(0x08000000 as *const u32);
        let reset_vector = core::ptr::read_volatile(0x08000004 as *const u32);

        // Valid bootloader should have:
        // - Stack pointer in RAM (0x20000000 - 0x20008000)
        // - Reset vector in flash (0x08000000 - 0x08040000) with Thumb bit set

        let sp_valid = stack_ptr >= 0x20000000 && stack_ptr <= 0x20008000;
        let reset_valid = (reset_vector & 0xFF000000) == 0x08000000 && (reset_vector & 1) == 1;

        sp_valid && reset_valid
    }
}

fn check_flash_base() -> bool {
    // Check if we can read a known valid flash address
    // If FLASH_BASE is corrupted, reads will fail or return garbage
    unsafe {
        // Read from a known location in the bootloader area
        // This should always be valid flash memory
        let test_addr = 0x08000000 as *const u32;
        let val = core::ptr::read_volatile(test_addr);

        // Stack pointer should be in RAM range, not 0xFFFFFFFF (erased)
        val != 0xFFFFFFFF && val >= 0x20000000 && val <= 0x20008000
    }
}

fn check_page_erased(page_addr: u32) -> bool {
    // Check if page is fully erased (all 0xFF)
    unsafe {
        let page_ptr = page_addr as *const u32;
        for i in 0..(PAGE_SIZE / 4) {
            if core::ptr::read_volatile(page_ptr.add(i)) != 0xFFFFFFFF {
                return false;
            }
        }
        true
    }
}
