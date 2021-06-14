use embassy_nrf::gpio::{FlexPin, OutputDrive, Pin, Pull};
use embedded_hal::digital::v2::{InputPin, OutputPin};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dht11Reading {
    pub temperature: i8,
    pub relative_humidity: u8,
}

impl Dht11Reading {
    fn raw_to_reading(bytes: [u8; 4]) -> Dht11Reading {
        let [rh, _, temp_signed, _] = bytes;
        let temp = {
            let (signed, magnitude) = convert_signed(temp_signed);
            let temp_sign = if signed { -1 } else { 1 };
            temp_sign * magnitude as i8
        };
        Dht11Reading {
            temperature: temp,
            relative_humidity: rh,
        }
    }

    pub fn read<'a, P>(pin: &mut FlexPin<'a, P>) -> Result<Dht11Reading, DhtError>
    where
        P: Pin,
    {
        let output = read(pin)?;
        Ok(Dht11Reading::raw_to_reading(output))
    }
}

fn read_bit<'a, P>(pin: &mut FlexPin<'a, P>) -> Result<bool, DhtError>
where
    P: Pin,
{
    let low = wait_until_timeout(|| pin.is_high(), 1000)?;
    let high = wait_until_timeout(|| pin.is_low(), 1000)?;
    Ok(high > low)
}

fn read_byte<'a, P>(pin: &mut FlexPin<'a, P>) -> Result<u8, DhtError>
where
    P: Pin,
{
    let mut byte: u8 = 0;
    for i in 0..8 {
        let bit_mask = 1 << (7 - (i % 8));
        if read_bit(pin)? {
            byte |= bit_mask;
        }
    }
    Ok(byte)
}

fn read<'a, P>(pin: &mut FlexPin<'a, P>) -> Result<[u8; 4], DhtError>
where
    P: Pin,
{
    pin.set_as_output(OutputDrive::Standard0Disconnect1);
    pin.set_high().ok();
    delay_ms(1);
    pin.set_low().ok();
    delay_ms(20);
    pin.set_high().ok();
    pin.set_as_input(Pull::Up);
    delay_us(40);

    read_bit(pin)?;

    log::info!("Start reading, reading input");

    let mut data = [0; 4];
    for b in data.iter_mut() {
        *b = read_byte(pin)?;
        log::info!("Read 0x{:x}", *b);
    }
    let checksum = read_byte(pin)?;
    log::info!("Checksum is 0x{:x}", checksum);
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
fn wait_until_timeout<E, F>(func: F, timeout_us: u32) -> Result<u32, DhtError>
where
    F: Fn() -> Result<bool, E>,
{
    let mut count = 0;
    for _ in 0..timeout_us {
        if func().ok().unwrap() {
            return Ok(count);
        }
        count += 1;
        delay_us(1);
    }
    Err(DhtError::Timeout)
}

fn convert_signed(signed: u8) -> (bool, u8) {
    let sign = signed & 0x80 != 0;
    let magnitude = signed & 0x7F;
    (sign, magnitude)
}

fn delay_ms(us: u32) {
    cortex_m::asm::delay(us * 64 * 1000);
}

fn delay_us(us: u32) {
    cortex_m::asm::delay(us * 64);
}
