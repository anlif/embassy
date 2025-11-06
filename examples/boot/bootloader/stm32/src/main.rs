#![no_std]
#![no_main]

use core::cell::RefCell;

use cortex_m_rt::{entry, exception};
#[cfg(feature = "defmt")]
use defmt_rtt as _;
#[cfg(feature = "defmt")]
use defmt::info;

use embassy_boot_stm32::*;
use embassy_stm32::flash::{BANK1_REGION, Flash};
use embassy_sync::blocking_mutex::Mutex;

#[entry]
fn main() -> ! {
    let p = embassy_stm32::init(Default::default());

    // Uncomment this if you are debugging the bootloader with debugger/RTT attached,
    // as it prevents a hard fault when accessing flash 'too early' after boot.
    for _i in 0..10000000 {
        cortex_m::asm::nop();
    }

    let layout = Flash::new_blocking(p.FLASH).into_blocking_regions();
    let flash = Mutex::new(RefCell::new(layout.bank1_region));

    let config = BootLoaderConfig::from_linkerfile_blocking(&flash, &flash, &flash);
    let active_offset = config.active.offset();
    #[cfg(feature = "defmt")]
    let state_offset = config.state.offset();

    #[cfg(feature = "defmt")]
    {
        info!("Bootloader starting");
        info!("ACTIVE partition offset: 0x{:x}", active_offset);
        info!("STATE partition offset: 0x{:x}", state_offset);

        // Read and display the state partition before processing
        use embedded_storage::nor_flash::ReadNorFlash;
        let mut state_buf = [0u8; 32];
        flash.lock(|f| {
            f.borrow_mut().read(state_offset, &mut state_buf).ok();
        });
        info!("STATE partition raw data (first 32 bytes): {:02x}", state_buf);
    }

    // Note: APP_A is now flashed directly to the ACTIVE partition by the flash tool
    // The bootloader simply loads and executes it
    #[cfg(feature = "defmt")]
    info!("Calling BootLoader::prepare...");

    let bl = BootLoader::prepare::<_, _, _, 2048>(config);

    #[cfg(feature = "defmt")]
    info!("BootLoader prepared, state: {:?}", bl.state);

    let boot_address = BANK1_REGION.base + active_offset;

    #[cfg(feature = "defmt")]
    {
        info!("Preparing to load from address 0x{:x}", boot_address);

        // Read the vector table to verify it's valid
        use core::ptr;
        unsafe {
            let stack_ptr = ptr::read_volatile(boot_address as *const u32);
            let reset_vector = ptr::read_volatile((boot_address + 4) as *const u32);

            info!("Vector table at 0x{:x}:", boot_address);
            info!("  Stack pointer: 0x{:08x}", stack_ptr);
            info!("  Reset vector:  0x{:08x}", reset_vector);

            // Sanity check: stack pointer should be in RAM (0x20000000 - 0x20008000)
            // Reset vector should be in flash (0x08000000 - 0x08040000) and odd (Thumb bit)
            if stack_ptr < 0x20000000 || stack_ptr > 0x20008000 {
                info!("WARNING: Stack pointer looks invalid!");
            }
            if (reset_vector & 0xFF000000) != 0x08000000 || (reset_vector & 1) == 0 {
                info!("WARNING: Reset vector looks invalid!");
            }
        }
    }

    #[cfg(feature = "defmt")]
    info!("Loading application...");

    unsafe { bl.load(boot_address) }
}

#[unsafe(no_mangle)]
#[cfg_attr(target_os = "none", unsafe(link_section = ".HardFault.user"))]
unsafe extern "C" fn HardFault() {
    #[cfg(feature = "defmt")]
    {
        defmt::error!("!!! HARDFAULT !!!");
        // Note: Cortex-M0+ doesn't have CFSR register
    }

    // Wait a bit for RTT to flush
    for _ in 0..1000000 {
        cortex_m::asm::nop();
    }

    cortex_m::peripheral::SCB::sys_reset();
}

#[exception]
unsafe fn DefaultHandler(_: i16) -> ! {
    const SCB_ICSR: *const u32 = 0xE000_ED04 as *const u32;
    let irqn = unsafe { core::ptr::read_volatile(SCB_ICSR) } as u8 as i16 - 16;

    panic!("DefaultHandler #{:?}", irqn);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    cortex_m::asm::udf();
}
