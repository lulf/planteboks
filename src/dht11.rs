use drogue_device::nrf::gpio::{FlexPin, OutputDrive, Pin, Pull};
use drogue_device::time::*;
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

    pub async fn read<'a, P>(pin: &mut FlexPin<'a, P>) -> Result<Dht11Reading, DhtError>
    where
        P: Pin,
    {
        let output = read(pin).await?;
        Ok(Dht11Reading::raw_to_reading(output))
    }
}

async fn read_bit<'a, P>(pin: &mut FlexPin<'a, P>) -> Result<bool, DhtError>
where
    P: Pin,
{
    wait_until_timeout(|| pin.is_high(), 100).await?;
    Timer::after(Duration::from_micros(35)).await;
    let high = pin.is_high().unwrap();
    wait_until_timeout(|| pin.is_low(), 100).await?;
    Ok(high)
}

async fn read_byte<'a, P>(pin: &mut FlexPin<'a, P>) -> Result<u8, DhtError>
where
    P: Pin,
{
    let mut byte: u8 = 0;
    for i in 0..8 {
        let bit_mask = 1 << (7 - (i % 8));
        if read_bit(pin).await? {
            byte |= bit_mask;
        }
    }
    Ok(byte)
}

async fn read<'a, P>(pin: &mut FlexPin<'a, P>) -> Result<[u8; 4], DhtError>
where
    P: Pin,
{
    pin.set_as_output(OutputDrive::Standard0Disconnect1);
    pin.set_low().ok();
    Timer::after(Duration::from_millis(18)).await;
    pin.set_high().ok();
    Timer::after(Duration::from_micros(48)).await;
    pin.set_as_input(Pull::Up);

    wait_until_timeout(|| pin.is_high(), 100).await?;
    wait_until_timeout(|| pin.is_low(), 100).await?;

    let mut data = [0; 4];
    for b in data.iter_mut() {
        *b = read_byte(pin).await?;
    }
    let checksum = read_byte(pin).await?;
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
    PinError,
}

/// Wait until the given function returns true or the timeout is reached.
async fn wait_until_timeout<E, F>(func: F, timeout_us: u8) -> Result<(), DhtError>
where
    F: Fn() -> Result<bool, E>,
{
    for _ in 0..timeout_us {
        if func().ok().unwrap() {
            return Ok(());
        }
        Timer::after(Duration::from_micros(1)).await;
    }
    Err(DhtError::Timeout)
}

fn convert_signed(signed: u8) -> (bool, u8) {
    let sign = signed & 0x80 != 0;
    let magnitude = signed & 0x7F;
    (sign, magnitude)
}
