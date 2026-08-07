//! Drive MCO1 on PA9 through a sweep of clock sources and prescalers.
//!
//! Every step is announced over defmt before it is applied, so a scope or logic
//! analyzer on PA9 can be checked against the expected frequency as the sweep
//! runs. Tested on a NUCLEO-C092RC.
//!
//! The clock tree is set up so that each measurement pins down a different
//! divider (RM0490, Figure 9):
//!
//!   HSI48 48 MHz ─┬─ HSIDIV /4 ─> HSISYS 12 MHz ─> [mux] ─> SYSDIV /3 ─> SYSCLK 4 MHz
//!                 └────────────────────────────────────────────> MCO mux
//!
//! MCO taps HSI48 *upstream* of HSIDIV, so `McoSource::Hsi` is the raw 48 MHz
//! no matter how HSIDIV is set; MCOPRE then divides it down. SYSCLK, by
//! contrast, has been through both dividers, and SYSDIV /3 puts it off the
//! power-of-two grid that HSIDIV and MCOPRE can reach on their own -- so a
//! SYSCLK step landing on 500 kHz confirms SYSDIV is being programmed.
//!
//! Every step is kept at or below 750 kHz, so a 24 MSa/s logic analyzer
//! (fx2lafw and friends) still gets 32 samples per period on the fastest one.

#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::Config;
use embassy_stm32::rcc::{Hsi, HsiDiv, HsiKerDiv, Mco, McoConfig, McoPrescaler, McoSource, SysDiv, Sysclk};
use embassy_time::Timer;
use panic_probe as _;

/// How long to hold each step, to leave time to read the scope.
const DWELL_MS: u64 = 200;

/// Length of the idle gap that separates one pass of the sweep from the next.
/// Short enough to read as a marker rather than as a step of its own.
const GAP_MS: u64 = 100;

/// (MCO source, MCO prescaler, expected frequency on PA9, what it pins down)
const SWEEP: &[(McoSource, McoPrescaler, &str, &str)] = &[
    (
        McoSource::Hsi,
        McoPrescaler::Div64,
        "750 kHz",
        "raw HSI48 upstream of HSIDIV -- HSISYS would read 187.5 kHz",
    ),
    (McoSource::Hsi, McoPrescaler::Div128, "375 kHz", "MCOPRE /128"),
    (
        McoSource::Hsi,
        McoPrescaler::Div1024,
        "46.875 kHz",
        "MCOPRE /1024, only on C051/C071/C09x",
    ),
    (
        McoSource::Sys,
        McoPrescaler::Div8,
        "500 kHz",
        "SYSCLK 4 MHz = 48 MHz / HSIDIV 4 / SYSDIV 3, then MCOPRE /8",
    ),
    (
        McoSource::Lsi,
        McoPrescaler::Div1,
        "~32 kHz",
        "LSI, untrimmed RC so expect wide tolerance",
    ),
];

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = Config::default();
    config.rcc.hse = None;
    config.rcc.hsi = Some(Hsi {
        div: HsiDiv::Div4,
        ker_div: HsiKerDiv::Div3,
    });
    config.rcc.sys = Sysclk::Hsisys;
    config.rcc.sys_div = SysDiv::Div3;
    config.rcc.ls.lsi = true;

    let p = embassy_stm32::init(config);

    let clocks = embassy_stm32::rcc::clocks(&p.RCC);
    info!(
        "sys={} Hz, hclk1={} Hz",
        clocks.sys.to_hertz().unwrap().0,
        clocks.hclk1.to_hertz().unwrap().0
    );

    let mut mco = p.MCO1;
    let mut pin = p.PA9;

    loop {
        // MCOSEL = disabled leaves the pin parked at a static level, so each
        // pass of the sweep starts with a flat gap that is easy to find in a
        // capture -- and easy to tell apart from the 32 kHz LSI step it follows.
        // Scoped so the reborrow is released before the sweep reclaims the pin.
        {
            let _mco = Mco::new(mco.reborrow(), pin.reborrow(), McoSource::Disable, McoConfig::default());
            info!("PA9: idle -- sweep restarts after this gap");
            Timer::after_millis(GAP_MS).await;
        }

        for (source, prescaler, expected, note) in SWEEP {
            let mut mco_config = McoConfig::default();
            mco_config.prescaler = *prescaler;

            // Dropped at the end of the iteration, which releases the reborrow
            // so the next step can reconfigure the same pin. MCO keeps driving
            // in the meantime -- there is no teardown on drop.
            let _mco = Mco::new(mco.reborrow(), pin.reborrow(), *source, mco_config);

            info!("PA9: {} -- {}", expected, note);
            Timer::after_millis(DWELL_MS).await;
        }
    }
}
