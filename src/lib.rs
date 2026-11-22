//! Controle dos LEDs RGB dos analogicos do RG52 Mini / RG43 Pro (AISLPC RK3562).
//!
//! O protocolo foi levantado por engenharia reversa pelo autor do `ledctl.sh`
//! (bmdhacks/dArkOS_rg52mini, `scripts/rk3562/ledctl.sh`) e reimplementado aqui:
//!
//! * a MCU dos LEDs fica na UART1 (`/dev/ttyS1`), 9600 8N1 raw, somente escrita;
//! * cada comando e **1 byte**, enviado `repeat` vezes (padrao 6) com ~10 ms de intervalo;
//! * handshake obrigatorio apos ligar o trilho: byte de init `0x20 + leds/4 - 1`;
//! * modo = o proprio numero do modo; brilho B (1..=9) = byte `50 - B`;
//! * o trilho `vcc-led` e ligado/desligado pelo debugfs do regulador.

pub mod config;
pub mod devices;
pub mod driver;
pub mod effects;
pub mod gfx;
pub mod header;
pub mod session;
pub mod ui;

pub use config::Config;
pub use driver::{
    bri_byte, init_byte, Controller, Desired, DryRun, Event, LedHandle, Options, Timing, Transport,
    UartTransport, BRI_MAX, BRI_MIN, DEFAULT_DEV, DEFAULT_PWR, MODE_OFF,
};
pub use effects::{decode, preview, Effect, Kind, BREATHING, SOLID};
pub use gfx::Color;
pub use session::{parse_byte, Session};
