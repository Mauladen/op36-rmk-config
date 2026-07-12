//! Right-half matrix of the 3W6HS: a TCA9555 I/O expander on I2C0.
//!
//! The right half mirrors the left's 4×5 matrix topology, but its row strobes
//! run through the expander's Port B (GPIOB0–B3, driven low to select a row)
//! and its columns are read from Port A (GPIOA0–A4). Port A is wired in
//! reverse on the PCB (QMK reference, commit d211040): GPIOA0 → col4 …
//! GPIOA4 → col0. Both halves are active low through COL2ROW diodes, so raw
//! reads are inverted before debouncing.
//!
//! Events are published as [`KeyboardEvent`]s with a row offset of 4, tiling
//! the right half onto keymap rows 4–7 below the left half's rows 0–3.
//!
//! Link robustness: every I2C transaction is fallible, so a failed transaction
//! skips the rest of the scan cycle and the expander initialization protocol
//! (direction registers + strobe release) is re-run before scanning again. A
//! disconnected or half-plugged cable thus degrades to "no right-half keys"
//! instead of a panic, and recovers without a reboot.

use defmt::warn;
use rmk::debounce::{DebounceState, DebouncerTrait};
use rmk::embassy_time::Timer;
use rmk::event::KeyboardEvent;
use rmk::macros::input_device;
use rmk::matrix::{KeyState, MatrixTrait};

/// TCA9555 7-bit address; all three address pins are tied to ground.
const TCA9555_ADDR: u8 = 0x20;
/// Command byte: Input Port 0 — right-half columns.
const REG_INPUT_PORT0: u8 = 0x00;
/// Command byte: Output Port 1 — right-half row strobes.
const REG_OUTPUT_PORT1: u8 = 0x03;
/// Command byte: Configuration Port 0 — the next byte configures Port 1.
const REG_CONFIG_PORT0: u8 = 0x06;
/// Port A direction: all eight pins are inputs (columns).
const CONFIG_PORT_A: u8 = 0xFF;
/// Port B direction: bits 0–3 outputs (row strobes), bits 4–7 inputs.
const CONFIG_PORT_B: u8 = 0xF0;
/// Keymap row of the first right-half row (published rows are 4–7).
const ROW_OFFSET: usize = 4;
/// Pause between expander re-init attempts while the link is down.
const RETRY_DELAY_MILLIS: u64 = 10;
/// Idle pause between full scan passes with no key change.
const SCAN_INTERVAL_MILLIS: u64 = 1;

/// Debounced scanner for the right-half matrix behind the TCA9555.
///
/// Generic over the I2C bus and the debouncer; `ROW`/`COL` describe the
/// expander grid (4×5 on the 3W6HS). State layout (`[col][row]`) and event
/// publishing follow `rmk::matrix::Matrix`, so the expander behaves like any
/// built-in RMK matrix.
#[input_device(publish = KeyboardEvent)]
pub struct Tca9555Matrix<I2C, D, const ROW: usize, const COL: usize>
where
    I2C: embedded_hal_async::i2c::I2c,
    I2C::Error: defmt::Format,
    D: DebouncerTrait<ROW, COL>,
{
    i2c: I2C,
    debouncer: D,
    /// Whether the expander direction registers are configured for scanning.
    initialized: bool,
    /// Debounced pressed state, indexed `[col][row]` like `rmk::matrix::Matrix`.
    key_states: [[KeyState; ROW]; COL],
    /// Scan resume point `(row, col)` after a published event.
    scan_pos: (usize, usize),
}

impl<I2C, D, const ROW: usize, const COL: usize> Tca9555Matrix<I2C, D, ROW, COL>
where
    I2C: embedded_hal_async::i2c::I2c,
    I2C::Error: defmt::Format,
    D: DebouncerTrait<ROW, COL>,
{
    /// Create the scanner. The expander is configured lazily on the first scan
    /// and re-configured after any I2C failure, so no bus traffic happens here.
    pub fn new(i2c: I2C, debouncer: D) -> Self {
        Self {
            i2c,
            debouncer,
            initialized: false,
            key_states: [[KeyState::new(); ROW]; COL],
            scan_pos: (0, 0),
        }
    }

    /// (Re-)configure the expander: Port A fully inputs, Port B low nibble
    /// outputs (IODIRA=0xFF, IODIRB=0xF0), then release all active-low strobes.
    async fn configure_expander(&mut self) -> Result<(), I2C::Error> {
        self.i2c
            .write(
                TCA9555_ADDR,
                &[REG_CONFIG_PORT0, CONFIG_PORT_A, CONFIG_PORT_B],
            )
            .await?;
        self.i2c
            .write(TCA9555_ADDR, &[REG_OUTPUT_PORT1, 0xFF])
            .await?;
        Ok(())
    }

    async fn read_keyboard_event(&mut self) -> KeyboardEvent {
        loop {
            if !self.initialized {
                match self.configure_expander().await {
                    Ok(()) => self.initialized = true,
                    Err(e) => {
                        warn!("TCA9555 init failed ({:?}), retrying", e);
                        Timer::after_millis(RETRY_DELAY_MILLIS).await;
                        continue;
                    }
                }
            }

            let (row_start, col_start) = self.scan_pos;
            let mut link_fault = false;

            'rows: for row in row_start..ROW {
                // Select the row: its strobe bit low, all others high.
                let strobe = !(1u8 << row);
                if let Err(e) = self
                    .i2c
                    .write(TCA9555_ADDR, &[REG_OUTPUT_PORT1, strobe])
                    .await
                {
                    warn!("TCA9555 row select failed ({:?})", e);
                    link_fault = true;
                    break 'rows;
                }
                // The strobe write over I2C doubles as the settle delay; no
                // extra wait is needed before sampling the columns.

                let mut port_a = [0u8; 1];
                if let Err(e) = self
                    .i2c
                    .write_read(TCA9555_ADDR, &[REG_INPUT_PORT0], &mut port_a)
                    .await
                {
                    warn!("TCA9555 column read failed ({:?})", e);
                    link_fault = true;
                    break 'rows;
                }
                // Columns are active low; invert so 1 = pressed.
                let cols = !port_a[0];

                let col_from = if row == row_start { col_start } else { 0 };
                for col in col_from..COL {
                    // GPIOA0 → col4 … GPIOA4 → col0 (reversed on the PCB).
                    let active = cols & (1 << (COL - 1 - col)) != 0;
                    let debounce_state = self.debouncer.detect_change_with_debounce(
                        row,
                        col,
                        active,
                        &self.key_states[col][row],
                    );
                    if let DebounceState::Debounced = debounce_state {
                        self.key_states[col][row].toggle_pressed();
                        self.scan_pos = (row, col);
                        return KeyboardEvent::key(
                            (row + ROW_OFFSET) as u8,
                            col as u8,
                            self.key_states[col][row].pressed,
                        );
                    }
                }
            }

            self.scan_pos = (0, 0);
            if link_fault {
                // Link dropped mid-scan: skip the rest of this cycle and
                // re-run the initialization protocol before scanning again.
                self.initialized = false;
                continue;
            }
            Timer::after_millis(SCAN_INTERVAL_MILLIS).await;
        }
    }
}

/// Without `async_matrix` (not enabled for this board) `MatrixTrait` has no
/// required methods; the impl marks the expander as a matrix like any built-in.
impl<I2C, D, const ROW: usize, const COL: usize> MatrixTrait<ROW, COL>
    for Tca9555Matrix<I2C, D, ROW, COL>
where
    I2C: embedded_hal_async::i2c::I2c,
    I2C::Error: defmt::Format,
    D: DebouncerTrait<ROW, COL>,
{
}
