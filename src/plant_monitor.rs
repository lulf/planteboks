use super::dht11::*;
use super::network::*;
use core::future::Future;

use core::pin::Pin;
use drogue_device::{
    actors::button::{ButtonEvent, FromButtonEvent},
    nrf::{
        gpio::FlexPin,
        peripherals::{P0_02, P0_04},
        saadc::*,
    },
    *,
};
use serde::Serialize;

#[derive(Clone, Copy)]
pub enum Command {
    TakeMeasurement,
}

impl<'a> FromButtonEvent<Command> for PlantMonitor<'a> {
    fn from(event: ButtonEvent) -> Option<Command> {
        match event {
            ButtonEvent::Pressed => None,
            ButtonEvent::Released => Some(Command::TakeMeasurement),
        }
    }
}

pub struct PlantMonitor<'a> {
    temperature: FlexPin<'a, P0_02>,
    soil: P0_04,
    adc: OneShot<'a>,
    network: Option<Address<'static, NetworkApi<'static>>>,
}

impl<'a> PlantMonitor<'a> {
    pub fn new(temperature: FlexPin<'a, P0_02>, soil: P0_04, adc: OneShot<'a>) -> Self {
        Self {
            network: None,
            temperature,
            soil,
            adc,
        }
    }

    async fn take_measurement(&mut self) -> Measurement {
        let mut measurement = Measurement {
            temperature: 0,
            humidity: 0,
            soil: 0,
        };

        log::info!("Take temperature measurement");
        match Dht11Reading::read(&mut self.temperature) {
            Ok(Dht11Reading {
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

impl<'a> Actor for PlantMonitor<'a> {
    type Configuration = Address<'static, NetworkApi<'static>>;
    #[rustfmt::skip]
    type Message<'m> where 'a: 'm = Command;
    #[rustfmt::skip]
    type OnStartFuture<'m> where 'a: 'm = impl Future<Output = ()> + 'm;
    #[rustfmt::skip]
    type OnMessageFuture<'m> where 'a: 'm = impl Future<Output = ()> + 'm;

    fn on_mount(&mut self, config: Self::Configuration) {
        self.network.replace(config);
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
                    this.network
                        .as_ref()
                        .unwrap()
                        .request(measurement)
                        .unwrap()
                        .await;
                }
            }
        }
    }
}

#[derive(Serialize)]
pub struct Measurement {
    soil: i16,
    temperature: i8,
    humidity: u8,
}
