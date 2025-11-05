#![no_std]
#![no_main]

#[cfg(feature = "defmt")]
use defmt_rtt::*;
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

    // WORKAROUND: Try using raw Flash with manual critical section
    // See FLASH_ERASE_BUG.md for details.
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

    #[cfg(feature = "defmt")]
    defmt::info!("Marking firmware as updated");

    updater.mark_updated().unwrap();
    led.set_low();
    Timer::after_secs(1).await;
    led.set_high();
    Timer::after_secs(3).await;
    led.set_low();
    Timer::after_secs(1).await;
    cortex_m::peripheral::SCB::sys_reset();
}
