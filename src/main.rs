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
mod network;
mod plant_monitor;
use network::*;
use plant_monitor::*;

use log::LevelFilter;
use panic_probe as _;
use rtt_logger::RTTLogger;
use rtt_target::rtt_init_print;

use drogue_device::{
    actors::{button::Button, ticker::Ticker},
    nrf::{
        buffered_uarte::BufferedUarte,
        gpio::{FlexPin, Input, Level, NoPin, Output, OutputDrive, Pull},
        gpiote::PortInput,
        interrupt,
        peripherals::P0_14,
        saadc::*,
        uarte, Peripherals,
    },
    time::*,
    traits::ip::*,
    *,
};

const WIFI_SSID: &str = include_str!(concat!(env!("OUT_DIR"), "/config/wifi.ssid.txt"));
const WIFI_PSK: &str = include_str!(concat!(env!("OUT_DIR"), "/config/wifi.password.txt"));
const HOST: IpAddress = IpAddress::new_v4(192, 168, 1, 2);
const PORT: u16 = 12345;

static LOGGER: RTTLogger = RTTLogger::new(LevelFilter::Info);

pub struct MyDevice {
    network: Network,
    monitor: ActorContext<'static, PlantMonitor<'static>>,
    ticker: ActorContext<'static, Ticker<'static, PlantMonitor<'static>>>,
    button:
        ActorContext<'static, Button<'static, PortInput<'static, P0_14>, PlantMonitor<'static>>>,
}

#[drogue::main]
async fn main(context: DeviceContext<MyDevice>, p: Peripherals) {
    rtt_init_print!();
    unsafe {
        log::set_logger_racy(&LOGGER).unwrap();
    }

    log::set_max_level(log::LevelFilter::Info);

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

    let temp_pin = FlexPin::new(p.P0_02);
    let soil_pin = p.P0_04;
    let adc = OneShot::new(p.SAADC, interrupt::take!(SAADC), Default::default());

    context.configure(MyDevice {
        ticker: ActorContext::new(Ticker::new(
            Duration::from_secs(300),
            Command::TakeMeasurement,
        )),
        button: ActorContext::new(Button::new(button_port)),
        network: Network::new(WIFI_SSID.trim_end(), WIFI_PSK.trim_end(), HOST, PORT),
        monitor: ActorContext::new(PlantMonitor::new(temp_pin, soil_pin, adc)),
    });

    context.mount(|device, spawner| {
        let network = device.network.mount((u, enable_pin, reset_pin), spawner);
        let monitor = device.monitor.mount(network, spawner);
        device.ticker.mount(monitor, spawner);
        device.button.mount(monitor, spawner);
    });
}
