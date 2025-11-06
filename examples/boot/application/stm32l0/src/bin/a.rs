#![no_std]
#![no_main]

#[cfg(feature = "defmt")]
use defmt_rtt as _;
use embassy_boot_stm32::{BlockingFirmwareUpdater, FirmwareUpdaterConfig};
use embassy_executor::Spawner;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::flash::{Flash, WRITE_SIZE};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::Timer;
use panic_reset as _;
use core::cell::RefCell;

#[cfg(feature = "skip-include")]
static APP_B: &[u8] = &[0, 1, 2, 3];
#[cfg(not(feature = "skip-include"))]
static APP_B: &[u8] = include_bytes!("../../b.bin");

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    let flash_periph = Flash::new_blocking(p.FLASH);
    let flash = Mutex::new(RefCell::new(flash_periph));

    let mut button = ExtiInput::new(p.PC13, p.EXTI13, Pull::Up);

    let mut led = Output::new(p.PC9, Level::Low, Speed::Low);

    led.set_high();
    Timer::after_secs(1).await;
    led.set_low();
    Timer::after_secs(1).await;
    led.set_high();

    button.wait_for_falling_edge().await;

    #[cfg(feature = "defmt")]
    defmt::info!("Button pressed, preparing firmware update");

    // Use blocking firmware updater
    let config = FirmwareUpdaterConfig::from_linkerfile_blocking(&flash, &flash);
    let mut magic = [0u8; WRITE_SIZE];
    let mut updater = BlockingFirmwareUpdater::new(config, &mut magic);

    #[cfg(feature = "defmt")]
    defmt::info!("Writing {} bytes to DFU partition", APP_B.len());

    let mut offset = 0;
    for (i, chunk) in APP_B.chunks(WRITE_SIZE).enumerate() {
        let mut buf = [0xFFu8; WRITE_SIZE];
        buf[..chunk.len()].copy_from_slice(chunk);

        #[cfg(feature = "defmt")]
        if i % 256 == 0 {
            defmt::info!("Writing chunk {} at offset 0x{:x}", i, offset);

            // Check if bootloader is still intact during writes
            use core::ptr;
            unsafe {
                let bootloader_sp = ptr::read_volatile(0x08000000 as *const u32);
                if bootloader_sp == 0xffffffff {
                    defmt::error!("BOOTLOADER ERASED at chunk {}!", i);
                    led.set_low();
                    loop {}
                }
            }
        }

        match updater.write_firmware(offset, &buf) {
            Ok(_) => {},
            Err(e) => {
                #[cfg(feature = "defmt")]
                defmt::error!("Write failed at chunk {}: {:?}", i, e);
                led.set_low();
                loop {}
            }
        }
        offset += chunk.len();
    }

    #[cfg(feature = "defmt")]
    defmt::info!("All {} chunks written successfully", APP_B.len() / WRITE_SIZE);

    // Check if bootloader is still intact before mark_updated
    #[cfg(feature = "defmt")]
    {
        use core::ptr;
        unsafe {
            let bootloader_sp = ptr::read_volatile(0x08000000 as *const u32);
            let bootloader_reset = ptr::read_volatile(0x08000004 as *const u32);
            defmt::info!("Before mark_updated - Bootloader SP: 0x{:08x}, Reset: 0x{:08x}", bootloader_sp, bootloader_reset);
        }
    }

    #[cfg(feature = "defmt")]
    defmt::info!("Marking firmware as updated");

    updater.mark_updated().unwrap();

    // Check if bootloader is still intact after mark_updated
    #[cfg(feature = "defmt")]
    {
        use core::ptr;
        unsafe {
            let bootloader_sp = ptr::read_volatile(0x08000000 as *const u32);
            let bootloader_reset = ptr::read_volatile(0x08000004 as *const u32);
            defmt::info!("After mark_updated - Bootloader SP: 0x{:08x}, Reset: 0x{:08x}", bootloader_sp, bootloader_reset);
        }
    }

    #[cfg(feature = "defmt")]
    defmt::info!("Mark updated completed");

    #[cfg(feature = "defmt")]
    defmt::info!("Waiting for flash controller to stabilize...");

    // CRITICAL: Give the flash controller time to complete all operations
    // STM32C0 flash controller may need time to finalize write operations
    led.set_low();
    Timer::after_millis(100).await;

    #[cfg(feature = "defmt")]
    defmt::info!("Flash controller stabilized");

    Timer::after_secs(1).await;
    led.set_high();
    Timer::after_secs(3).await;
    led.set_low();
    Timer::after_secs(1).await;

    #[cfg(feature = "defmt")]
    defmt::info!("Resetting from a");

    cortex_m::peripheral::SCB::sys_reset();
}
