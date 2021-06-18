use super::dht11::{self, Delay};
use core::future::Future;

use core::pin::Pin;
use drogue_device::{
    actors::button::{ButtonEvent, FromButtonEvent},
    *,
};
use embassy_nrf::{
    gpio::FlexPin,
    peripherals::{P0_02, P0_04},
    saadc::*,
};
use serde::Serialize;

#[derive(Clone, Copy)]
pub enum Command {
    TakeMeasurement,
}

#[rustfmt::skip]
impl<'a, A, D> FromButtonEvent<Command> for PlantMonitor<'a, A, D>
where
    A: Actor<Message<'a> = Measurement> + 'static,
    D: Delay + 'a,
{
    fn from(event: ButtonEvent) -> Option<Command> {
        match event {
            ButtonEvent::Pressed => None,
            ButtonEvent::Released => Some(Command::TakeMeasurement),
        }
    }
}

#[rustfmt::skip]
pub struct PlantMonitor<'a, A, D>
where
    A: Actor<Message<'a> = Measurement> + 'static,
    D: Delay + 'static,
{
    delay: D,
    temperature: FlexPin<'a, P0_02>,
    soil: P0_04,
    adc: OneShot<'a>,
    sink: Option<Address<'a, A>>,
}

#[rustfmt::skip]
impl<'a, A, D> PlantMonitor<'a, A, D>
where
    A: Actor<Message<'a> = Measurement> + 'static,
    D: Delay + 'a,
{
    pub fn new(temperature: FlexPin<'a, P0_02>, soil: P0_04, adc: OneShot<'a>, delay: D) -> Self {
        Self {
            sink: None,
            delay,
            temperature,
            soil,
            adc,
        }
    }

    async fn report_measurement<'m>(&mut self, measurement: Measurement)
    {
        self.sink.unwrap().request(measurement).unwrap().await;
    }

    async fn take_measurement(&mut self) -> Measurement {
        let mut measurement = Measurement {
            temperature: 0,
            humidity: 0,
            soil: 0,
        };

        let delay = &mut self.delay;

        log::info!("Take temperature measurement");
        match dht11::read(delay, &mut self.temperature) {
            Ok(dht11::Reading {
                temperature,
                relative_humidity,
            }) => {
                log::info!(
                    "Got temperature: {}. Humidity: {}",
                    temperature,
                    relative_humidity,
                );
                measurement.temperature = temperature;
                measurement.humidity = relative_humidity;
            }
            Err(e) => log::warn!("Error getting temperature reading: {:?}", e),
        }

        let sample = Pin::new(&mut self.adc).sample(&mut self.soil).await;
        log::info!("Got soil sample: {}", sample);
        measurement.soil = sample;
        measurement
    }
}

#[rustfmt::skip]
impl<'a, A, D> Actor for PlantMonitor<'a, A, D>
where
    A: Actor<Message<'a> = Measurement> + 'a,
    D: Delay + 'static,
{
    type Configuration = Address<'a, A>;
    #[rustfmt::skip]
    type Message<'m> where 'a: 'm = Command;
    #[rustfmt::skip]
    type OnStartFuture<'m> where 'a: 'm = impl Future<Output = ()> + 'm;
    #[rustfmt::skip]
    type OnMessageFuture<'m> where 'a: 'm = impl Future<Output = ()> + 'm;

    fn on_mount(&mut self, _: Address<'static, Self>, config: Self::Configuration) {
        self.sink.replace(config);
    }

    fn on_start<'m>(self: Pin<&'m mut Self>) -> Self::OnStartFuture<'m> {
        async move {}
    }

    fn on_message<'m>(
        self: Pin<&'m mut Self>,
        message: Self::Message<'m>,
    ) -> Self::OnMessageFuture<'m> {
        async move {
            let this = unsafe { self.get_unchecked_mut() };
            match message {
                Command::TakeMeasurement => {
                    let measurement = this.take_measurement().await;
                    this.report_measurement(measurement).await;
                }
            }
        }
    }
}

#[derive(Serialize, Clone, Copy)]
pub struct Measurement {
    pub soil: i16,
    pub temperature: i8,
    pub humidity: u8,
}
