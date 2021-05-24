use crate::plant_monitor::Measurement;
use core::cell::UnsafeCell;
use core::future::Future;

use core::pin::Pin;
use drogue_device::{
    drivers::wifi::esp8266::*,
    nrf::{
        buffered_uarte::BufferedUarte,
        gpio::Output,
        peripherals::{P0_09, P0_10, TIMER0, UARTE0},
    },
    traits::{ip::*, tcp::*, wifi::*},
    *,
};

use serde::Serialize;
use serde_cbor::ser::SliceWrite;
use serde_cbor::Serializer;

type UART = BufferedUarte<'static, UARTE0, TIMER0>;
type ENABLE = Output<'static, P0_09>;
type RESET = Output<'static, P0_10>;

pub struct Network {
    driver: UnsafeCell<Esp8266Driver>,
    api: ActorContext<'static, NetworkApi<'static>>,
    modem: ActorContext<'static, Esp8266ModemActor<'static, UART, ENABLE, RESET>>,
}

impl Network {
    pub fn new(ssid: &'static str, psk: &'static str, ip: IpAddress, port: u16) -> Self {
        Self {
            driver: UnsafeCell::new(Esp8266Driver::new()),
            api: ActorContext::new(NetworkApi::new(ssid, psk, ip, port)),
            modem: ActorContext::new(Esp8266ModemActor::new()),
        }
    }
}

impl Package for Network {
    type Configuration = (UART, ENABLE, RESET);
    type Primary = NetworkApi<'static>;
    fn mount(
        &'static self,
        config: Self::Configuration,
        spawner: &ActorSpawner,
    ) -> Address<Self::Primary> {
        let (controller, modem) =
            unsafe { &mut *self.driver.get() }.initialize(config.0, config.1, config.2);
        self.modem.mount(modem, spawner);
        self.api.mount(controller, spawner)
    }
}

pub struct NetworkApi<'a> {
    ssid: &'static str,
    psk: &'static str,
    ip: IpAddress,
    port: u16,
    driver: Option<Esp8266Controller<'a>>,
    socket: Option<<Esp8266Controller<'a> as TcpStack>::SocketHandle>,
}

impl<'a> NetworkApi<'a> {
    pub fn new(ssid: &'static str, psk: &'static str, ip: IpAddress, port: u16) -> Self {
        Self {
            ssid,
            psk,
            ip,
            port,
            socket: None,
            driver: None,
        }
    }
}

impl<'a> Actor for NetworkApi<'a> {
    type Configuration = Esp8266Controller<'a>;
    #[rustfmt::skip]
    type Message<'m> where 'a: 'm = Measurement;
    #[rustfmt::skip]
    type OnStartFuture<'m> where 'a: 'm = impl Future<Output = ()> + 'm;
    #[rustfmt::skip]
    type OnMessageFuture<'m> where 'a: 'm = impl Future<Output = ()> + 'm;

    fn on_mount(&mut self, config: Self::Configuration) {
        self.driver.replace(config);
    }

    fn on_start<'m>(self: Pin<&'m mut Self>) -> Self::OnStartFuture<'m> {
        async move {
            let this = unsafe { self.get_unchecked_mut() };
            let driver = this.driver.as_mut().unwrap();
            log::info!("Joining access point");
            driver
                .join(Join::Wpa {
                    ssid: this.ssid,
                    password: this.psk,
                })
                .await
                .expect("Error joining wifi");
            log::info!("Joined access point");

            let socket = driver.open().await;

            log::info!("Connecting to {}:{}", this.ip, this.port);
            let result = driver
                .connect(
                    socket,
                    IpProtocol::Tcp,
                    SocketAddress::new(this.ip, this.port),
                )
                .await;
            match result {
                Ok(_) => {
                    this.socket.replace(socket);
                    log::info!("Connected to {:?}!", this.ip);
                }
                Err(e) => {
                    log::warn!("Error connecting: {:?}", e);
                }
            }
        }
    }

    fn on_message<'m>(
        self: Pin<&'m mut Self>,
        message: Self::Message<'m>,
    ) -> Self::OnMessageFuture<'m> {
        async move {
            let this = unsafe { self.get_unchecked_mut() };
            let mut driver = this.driver.take().expect("driver not bound!");
            let socket = this.socket.take().expect("socket not bound!");

            let mut buf = [0; 256];
            let writer = SliceWrite::new(&mut buf[..]);
            let mut ser = Serializer::new(writer);
            match message.serialize(&mut ser) {
                Ok(_) => {
                    let writer = ser.into_inner();
                    let size = writer.bytes_written();
                    let result = driver.write(socket, &buf[..size]).await;
                    match result {
                        Ok(_) => {
                            log::debug!("Measurement reported");
                        }
                        Err(e) => {
                            log::warn!("Error reporting measurement: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Error serializing measurement: {:?}", e);
                }
            }
            this.driver.replace(driver);
            this.socket.replace(socket);
        }
    }
}
