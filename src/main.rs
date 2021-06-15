#![no_std]
#![no_main]
#![macro_use]
#![allow(incomplete_features)]
#![feature(generic_associated_types)]
#![feature(min_type_alias_impl_trait)]
#![feature(impl_trait_in_bindings)]
#![feature(type_alias_impl_trait)]
#![feature(concat_idents)]

mod delay;
mod dht11;
mod http;
mod network;
mod plant_monitor;
mod splitter;
use delay::*;
use network::*;
use plant_monitor::*;
use splitter::*;

use log::LevelFilter;
use panic_probe as _;
use rtt_logger::RTTLogger;
use rtt_target::rtt_init_print;

use drogue_device::{
    actors::{
        button::Button,
        ticker::Ticker,
        wifi::{esp8266::*, *},
    },
    drivers::wifi::esp8266::*,
    traits::{ip::*, wifi::Join},
    *,
};

use embassy::time::Duration;

use embassy_nrf::{
    buffered_uarte::BufferedUarte,
    gpio::{FlexPin, Input, Level, NoPin, Output, OutputDrive, Pull},
    gpiote::PortInput,
    interrupt,
    peripherals::{P0_09, P0_10, P0_14, TIMER0, UARTE0},
    saadc::*,
    uarte, Peripherals,
};

use serde::Serialize;

const WIFI_SSID: &str = include_str!(concat!(env!("OUT_DIR"), "/config/wifi.ssid.txt"));
const WIFI_PSK: &str = include_str!(concat!(env!("OUT_DIR"), "/config/wifi.password.txt"));
const HOST: IpAddress = IpAddress::new_v4(192, 168, 1, 2);
const PORT: u16 = 5000;
const PUBLIC_USERNAME: &str = include_str!(concat!(env!("OUT_DIR"), "/config/public.username.txt"));
const PUBLIC_PASSWORD: &str = include_str!(concat!(env!("OUT_DIR"), "/config/public.password.txt"));

// const GEOLOC: &str = include_str!(concat!(env!("OUT_DIR"), "/config/geolocation.txt"));
const GEOLOC_LAT: f32 = 60.795974;
const GEOLOC_LON: f32 = 11.076333;

static LOGGER: RTTLogger = RTTLogger::new(LevelFilter::Info);

type UART = BufferedUarte<'static, UARTE0, TIMER0>;
type ENABLE = Output<'static, P0_09>;
type RESET = Output<'static, P0_10>;
type WifiDriver = Esp8266Controller<'static>;

#[derive(Serialize)]
pub struct SandboxMeasurement {
    temp: i8,
    hum: u8,
    geoloc: Geolocation,
}

#[derive(Serialize)]
pub struct Geolocation {
    lon: f32,
    lat: f32,
}

impl From<Measurement> for SandboxMeasurement {
    fn from(m: Measurement) -> SandboxMeasurement {
        SandboxMeasurement {
            temp: m.temperature,
            hum: m.humidity,
            geoloc: Geolocation {
                lon: GEOLOC_LON,
                lat: GEOLOC_LAT,
            },
        }
    }
}

type PublicApi = NetworkEndpoint<'static, WifiDriver, SandboxMeasurement>;
type PrivateApi = NetworkEndpoint<'static, WifiDriver, Measurement>;
type Monitor = PlantMonitor<'static, PublicApi, Delay>; //Splitter<'static, Measurement, PublicApi, PrivateApi>, Delay>;

pub struct MyDevice {
    wifi: Esp8266Wifi<UART, ENABLE, RESET>,
    public: ActorContext<'static, PublicApi>,
    //private: ActorContext<'static, PrivateApi>,
    //splitter: ActorContext<'static, Splitter<'static, Measurement, PublicApi, PrivateApi>>,
    monitor: ActorContext<'static, Monitor>,
    ticker: ActorContext<'static, Ticker<'static, Monitor>>,
    button: ActorContext<'static, Button<'static, PortInput<'static, P0_14>, Monitor>>,
}

static DEVICE: DeviceContext<MyDevice> = DeviceContext::new();

#[embassy::main]
async fn main(spawner: embassy::executor::Spawner, p: Peripherals) {
    rtt_init_print!();
    log::set_logger(&LOGGER).unwrap();
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

    let cp = unsafe { cortex_m::Peripherals::steal() };

    DEVICE.configure(MyDevice {
        ticker: ActorContext::new(Ticker::new(
            Duration::from_secs(300),
            Command::TakeMeasurement,
        )),
        button: ActorContext::new(Button::new(button_port)),
        wifi: Esp8266Wifi::new(u, enable_pin, reset_pin),
        public: ActorContext::new(NetworkEndpoint::new(
            HOST,
            PORT,
            PUBLIC_USERNAME.trim_end(),
            PUBLIC_PASSWORD.trim_end(),
        )),
        /*
        private: ActorContext::new(NetworkEndpoint::new(
            HOST,
            PORT,
            PUBLIC_USERNAME,
            PUBLIC_PASSWORD,
        )),
        splitter: ActorContext::new(Splitter::new()),*/
        monitor: ActorContext::new(PlantMonitor::new(
            temp_pin,
            soil_pin,
            adc,
            Delay::new(cp.SYST),
        )),
    });

    let wifi = DEVICE.mount(|device| {
        let wifi = device.wifi.mount((), spawner);
        let public = device.public.mount(WifiAdapter::new(wifi), spawner);
        //let private = device.private.mount(WifiAdapter::new(wifi), spawner);
        //let splitter = device.splitter.mount((public, private), spawner);
        // let monitor = device.monitor.mount(splitter, spawner);
        let monitor = device.monitor.mount(public, spawner);
        device.ticker.mount(monitor, spawner);
        device.button.mount(monitor, spawner);
        WifiAdapter::new(wifi)
    });

    let ssid = WIFI_SSID.trim_end();
    let password = WIFI_PSK.trim_end();

    log::info!("Joining access point");
    wifi.join(Join::Wpa { ssid, password })
        .await
        .expect("Error joining wifi");
    log::info!("Joined access point");
}
