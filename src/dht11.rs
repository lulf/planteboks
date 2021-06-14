use embassy_nrf::gpio::{FlexPin, OutputDrive, Pin, Pull};
use embedded_hal::{
    blocking::delay::{DelayMs, DelayUs},
    digital::v2::{InputPin, OutputPin},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub temperature: i8,
    pub relative_humidity: u8,
}

pub trait Delay: DelayUs<u32> + DelayMs<u32> {}
impl<T> Delay for T where T: DelayMs<u32> + DelayUs<u32> {}

fn raw_to_reading(bytes: [u8; 4]) -> Reading {
    let [rh, _, temp_signed, _] = bytes;
    let temp = {
        let (signed, magnitude) = convert_signed(temp_signed);
        let temp_sign = if signed { -1 } else { 1 };
        temp_sign * magnitude as i8
    };
    Reading {
        temperature: temp,
        relative_humidity: rh,
    }
}

pub fn read<'a, P, D>(delay: &mut D, pin: &mut FlexPin<'a, P>) -> Result<Reading, DhtError>
where
    P: Pin,
    D: Delay,
{
    let output = read_raw(delay, pin)?;
    Ok(raw_to_reading(output))
}

fn read_bit<'a, P, D>(delay: &mut D, pin: &mut FlexPin<'a, P>) -> Result<bool, DhtError>
where
    P: Pin,
    D: Delay,
{
    let low = wait_until_timeout(delay, || pin.is_high(), 1000)?;
    let high = wait_until_timeout(delay, || pin.is_low(), 1000)?;
    Ok(high > low)
}

fn read_byte<'a, P, D>(delay: &mut D, pin: &mut FlexPin<'a, P>) -> Result<u8, DhtError>
where
    P: Pin,
    D: Delay,
{
    let mut byte: u8 = 0;
    for i in 0..8 {
        let bit_mask = 1 << (7 - (i % 8));
        if read_bit(delay, pin)? {
            byte |= bit_mask;
        }
    }
    Ok(byte)
}

fn read_raw<'a, P, D>(delay: &mut D, pin: &mut FlexPin<'a, P>) -> Result<[u8; 4], DhtError>
where
    P: Pin,
    D: Delay,
{
    pin.set_as_output(OutputDrive::Standard0Disconnect1);
    pin.set_high().ok();
    delay.delay_ms(1);
    pin.set_low().ok();
    delay.delay_ms(18);
    pin.set_high().ok();
    pin.set_as_input(Pull::Up);
    delay.delay_us(48);

    read_bit(delay, pin)?;

    let mut data = [0; 4];
    for b in data.iter_mut() {
        *b = read_byte(delay, pin)?;
    }
    let checksum = read_byte(delay, pin)?;
    if data.iter().fold(0u8, |sum, v| sum.wrapping_add(*v)) != checksum {
        Err(DhtError::ChecksumMismatch)
    } else {
        Ok(data)
    }
}

#[derive(Debug)]
pub enum DhtError {
    ChecksumMismatch,
    Timeout,
}

/// Wait until the given function returns true or the timeout is reached.
fn wait_until_timeout<D, E, F>(delay: &mut D, func: F, timeout_us: u32) -> Result<u32, DhtError>
where
    D: Delay,
    F: Fn() -> Result<bool, E>,
{
    let mut count = 0;
    for _ in 0..timeout_us {
        if func().ok().unwrap() {
            return Ok(count);
        }
        count += 1;
        delay.delay_us(1);
    }
    Err(DhtError::Timeout)
}

fn convert_signed(signed: u8) -> (bool, u8) {
    let sign = signed & 0x80 != 0;
    let magnitude = signed & 0x7F;
    (sign, magnitude)
}

/*
#[inline]
fn delay_ms(us: u32) {
    cortex_m::asm::delay(us * 64 * 1000);
}

#[inline]
fn delay_us(us: u32) {
    cortex_m::asm::delay(us * 64);
}
*/
