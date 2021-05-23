#![no_std]
#![no_main]
#![macro_use]
#![allow(incomplete_features)]
#![feature(generic_associated_types)]
#![feature(min_type_alias_impl_trait)]
#![feature(impl_trait_in_bindings)]
#![feature(type_alias_impl_trait)]
#![feature(concat_idents)]

mod dht11;
use dht11::*;
mod app;
use app::*;

use log::LevelFilter;
use panic_probe as _;
use rtt_logger::RTTLogger;
use rtt_target::rtt_init_print;

use core::cell::UnsafeCell;
use core::pin::Pin;
use drogue_device::{
    actors::button::Button,
    drivers::wifi::esp8266::*,
    nrf::{
        buffered_uarte::BufferedUarte,
        gpio::{FlexPin, Input, Level, NoPin, Output, OutputDrive, Pull},
        gpiote::{self, PortInput},
        interrupt,
        peripherals::{P0_02, P0_03, P0_09, P0_10, P0_14, TIMER0, UARTE0},
        saadc::*,
        uarte, Peripherals,
    },
    time::*,
    traits::ip::*,
    *,
};
use embedded_hal::digital::v2::{InputPin, OutputPin};

const WIFI_SSID: &str = include_str!(concat!(env!("OUT_DIR"), "/config/wifi.ssid.txt"));
const WIFI_PSK: &str = include_str!(concat!(env!("OUT_DIR"), "/config/wifi.password.txt"));
const HOST: IpAddress = IpAddress::new_v4(192, 168, 1, 2);
const PORT: u16 = 12345;

static LOGGER: RTTLogger = RTTLogger::new(LevelFilter::Info);

type UART = BufferedUarte<'static, UARTE0, TIMER0>;
type ENABLE = Output<'static, P0_09>;
type RESET = Output<'static, P0_10>;

pub struct MyDevice {
    driver: UnsafeCell<Esp8266Driver>,
    modem: ActorContext<'static, Esp8266ModemActor<'static, UART, ENABLE, RESET>>,
    app: ActorContext<'static, App<Esp8266Controller<'static>>>,
    button: ActorContext<
        'static,
        Button<'static, PortInput<'static, P0_14>, App<Esp8266Controller<'static>>>,
    >,
}

#[drogue::main]
async fn main(context: DeviceContext<MyDevice>, p: Peripherals) {
    rtt_init_print!();
    unsafe {
        log::set_logger_racy(&LOGGER).unwrap();
    }

    log::set_max_level(log::LevelFilter::Info);

    //    let p = Peripherals::take().unwrap();
    //    let g = gpiote::initialize(p.GPIOTE, interrupt::take!(GPIOTE));

    let button_port = PortInput::new(Input::new(p.P0_14, Pull::Up));

    let mut config = uarte::Config::default();
    config.parity = uarte::Parity::EXCLUDED;
    config.baudrate = uarte::Baudrate::BAUD115200;

    static mut TX_BUFFER: [u8; 256] = [0u8; 256];
    static mut RX_BUFFER: [u8; 256] = [0u8; 256];

    let irq = interrupt::take!(UARTE0_UART0);
    let u = unsafe {
        BufferedUarte::new(
            p.UARTE0,
            p.TIMER0,
            p.PPI_CH0,
            p.PPI_CH1,
            irq,
            p.P0_13,
            p.P0_01,
            NoPin,
            NoPin,
            config,
            &mut RX_BUFFER,
            &mut TX_BUFFER,
        )
    };

    let enable_pin = Output::new(p.P0_09, Level::Low, OutputDrive::Standard);
    let reset_pin = Output::new(p.P0_10, Level::Low, OutputDrive::Standard);

    let mut temp_pin = FlexPin::new(p.P0_02);
    temp_pin.set_high().unwrap();

    context.configure(MyDevice {
        driver: UnsafeCell::new(Esp8266Driver::new()),
        modem: ActorContext::new(Esp8266ModemActor::new()),
        app: ActorContext::new(App::new(
            WIFI_SSID.trim_end(),
            WIFI_PSK.trim_end(),
            HOST,
            PORT,
        )),
        button: ActorContext::new(Button::new(button_port)),
    });

    context.mount(|device, spawner| {
        let (controller, modem) =
            unsafe { &mut *device.driver.get() }.initialize(u, enable_pin, reset_pin);
        device.modem.mount(modem, spawner);
        let app = device.app.mount(controller, spawner);
        device.button.mount(app, spawner);
    });

    let mut soil_pin = p.P0_04;
    let mut adc = OneShot::new(p.SAADC, interrupt::take!(SAADC), Default::default());

    loop {
        Timer::after(Duration::from_secs(1)).await;
        let sample = Pin::new(&mut adc).sample(&mut soil_pin).await;
        log::info!("Got soil sample: {}", sample);
        match Dht11Reading::read(&mut temp_pin).await {
            Ok(Dht11Reading {
                temperature,
                relative_humidity,
            }) => log::info!(
                "Got temperature: {}. Humidity: {}",
                temperature,
                relative_humidity
            ),
            Err(e) => log::info!("Error getting temperature reading: {:?}", e),
        }
    }
}
