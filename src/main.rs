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
mod display;
mod network;
mod plant_monitor;
mod rng;
mod splitter;
use delay::*;
use display::*;
use network::*;
use plant_monitor::*;
use rng::*;
use splitter::*;

use panic_reset as _;
//use log::LevelFilter;
//use panic_probe as _;
//use rtt_logger::RTTLogger;
//use rtt_target::rtt_init_print;

use drogue_device::{
    actors::{button::Button, socket::*, ticker::Ticker, wifi::esp8266::*},
    drivers::wifi::esp8266::*,
    traits::{ip::*, tcp::*, wifi::*},
    *,
};
use drogue_tls::*;

use embassy::time::Duration;

use embassy_nrf::{
    buffered_uarte::BufferedUarte,
    gpio::{AnyPin, FlexPin, Input, Level, NoPin, Output, OutputDrive, Pin, Pull},
    gpiote::PortInput,
    interrupt,
    peripherals::{P0_09, P0_10, P0_14, TIMER0, UARTE0},
    saadc::*,
    uarte, Peripherals,
};
use nrf52833_pac as pac;

const WIFI_SSID: &str = include_str!(concat!(env!("OUT_DIR"), "/config/wifi.ssid.txt"));
const WIFI_PSK: &str = include_str!(concat!(env!("OUT_DIR"), "/config/wifi.password.txt"));

const HOST: &str = "http.sandbox.drogue.cloud";
const IP: IpAddress = IpAddress::new_v4(95, 216, 224, 167); // IP resolved for "http.sandbox.drogue.cloud"
const PORT: u16 = 5000;

const USERNAME: &str = include_str!(concat!(env!("OUT_DIR"), "/config/username.txt"));
const PASSWORD: &str = include_str!(concat!(env!("OUT_DIR"), "/config/password.txt"));

// static LOGGER: RTTLogger = RTTLogger::new(LevelFilter::Info);

type UART = BufferedUarte<'static, UARTE0, TIMER0>;
type ENABLE = Output<'static, P0_09>;
type RESET = Output<'static, P0_10>;
type AppSocket =
    TlsSocket<'static, Socket<'static, Esp8266Controller<'static>>, Rng, Aes128GcmSha256>;

type Network = NetworkEndpoint<AppSocket, Measurement>;
type Sink = Splitter<'static, Measurement, Network, <Display as Package>::Primary>;
type Monitor = PlantMonitor<'static, Sink, Delay>;

pub struct MyDevice {
    wifi: Esp8266Wifi<UART, ENABLE, RESET>,
    display: Display,
    network: ActorContext<'static, Network>,
    sink: ActorContext<'static, Sink>,
    monitor: ActorContext<'static, Monitor>,
    ticker: ActorContext<'static, Ticker<'static, Monitor>>,
    button: ActorContext<'static, Button<'static, PortInput<'static, P0_14>, Monitor>>,
}

static mut TLS_BUFFER: [u8; 16384] = [0; 16384];
static DEVICE: DeviceContext<MyDevice> = DeviceContext::new();

fn output_pin(pin: AnyPin) -> Output<'static, AnyPin> {
    Output::new(pin, Level::Low, OutputDrive::Standard)
}

#[embassy::main]
async fn main(spawner: embassy::executor::Spawner, p: Peripherals) {
    //rtt_init_print!();
    //log::set_logger(&LOGGER).unwrap();
    //log::set_max_level(log::LevelFilter::Info);

    let button_port = PortInput::new(Input::new(p.P0_14, Pull::Up));

    let mut config = uarte::Config::default();
    config.parity = uarte::Parity::EXCLUDED;
    config.baudrate = uarte::Baudrate::BAUD115200;

    static mut TX_BUFFER: [u8; 8192] = [0u8; 8192];
    static mut RX_BUFFER: [u8; 8192] = [0u8; 8192];

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

    // LED Matrix
    let rows = [
        output_pin(p.P0_21.degrade()),
        output_pin(p.P0_22.degrade()),
        output_pin(p.P0_15.degrade()),
        output_pin(p.P0_24.degrade()),
        output_pin(p.P0_19.degrade()),
    ];

    let cols = [
        output_pin(p.P0_28.degrade()),
        output_pin(p.P0_11.degrade()),
        output_pin(p.P0_31.degrade()),
        output_pin(p.P1_05.degrade()),
        output_pin(p.P0_30.degrade()),
    ];

    let enable_pin = Output::new(p.P0_09, Level::Low, OutputDrive::Standard);
    let reset_pin = Output::new(p.P0_10, Level::Low, OutputDrive::Standard);

    let temp_pin = FlexPin::new(p.P0_02);
    let soil_pin = p.P0_04;
    let adc = OneShot::new(p.SAADC, interrupt::take!(SAADC), Default::default());

    let cp = unsafe { cortex_m::Peripherals::steal() };

    DEVICE.configure(MyDevice {
        ticker: ActorContext::new(Ticker::new(
            Duration::from_secs(600),
            Command::TakeMeasurement,
        )),
        button: ActorContext::new(Button::new(button_port)),
        wifi: Esp8266Wifi::new(u, enable_pin, reset_pin),
        network: ActorContext::new(NetworkEndpoint::new(
            IP,
            PORT,
            USERNAME.trim_end(),
            PASSWORD.trim_end(),
        )),
        sink: ActorContext::new(Splitter::new()),
        monitor: ActorContext::new(PlantMonitor::new(
            temp_pin,
            soil_pin,
            adc,
            Delay::new(cp.SYST),
        )),
        display: Display::new(rows, cols),
    });

    DEVICE
        .mount(|device| async move {
            let display = device.display.mount((), spawner);
            let mut wifi = device.wifi.mount((), spawner);
            wifi.join(Join::Wpa {
                ssid: WIFI_SSID.trim_end(),
                password: WIFI_PSK.trim_end(),
            })
            .await
            .expect("Error joining wifi");
            log::info!("Joined access point");

            let socket = Socket::new(wifi, wifi.open().await);
            let socket = TlsSocket::wrap(
                socket,
                TlsContext::new(Rng::new(pac::Peripherals::take().unwrap().RNG), unsafe {
                    &mut TLS_BUFFER
                })
                .with_server_name(HOST.trim_end()),
            );
            let network = device.network.mount(socket, spawner);
            let sink = device.sink.mount((network, display), spawner);
            let monitor = device.monitor.mount(sink, spawner);
            device.ticker.mount(monitor, spawner);
            device.button.mount(monitor, spawner);
        })
        .await;
}
