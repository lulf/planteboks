use embassy_nrf::gpio::{FlexPin, OutputDrive, Pin, Pull};
use embedded_hal::{
    blocking::delay::{DelayMs, DelayUs},
    digital::v2::{InputPin, OutputPin},
};

#[derive(Clone, Copy)]
pub struct Reading {
    pub temperature: f32,
    pub relative_humidity: f32,
}

pub trait Delay: DelayUs<u32> + DelayMs<u32> {}
impl<T> Delay for T where T: DelayMs<u32> + DelayUs<u32> {}

fn raw_to_reading(bytes: [u8; 4]) -> Reading {
    let mut temp = i16::from(bytes[2] & 0x7f) * 10 + i16::from(bytes[3]);
    if bytes[2] & 0x80 != 0 {
        temp = -temp;
    }

    let hum = u16::from(bytes[0]) * 10 + u16::from(bytes[1]);

    Reading {
        temperature: (temp as f32) / 10.0,
        relative_humidity: (hum as f32) / 10.0,
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
