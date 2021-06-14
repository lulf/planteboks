use crate::plant_monitor::Measurement;
use core::future::Future;

use core::pin::Pin;
use drogue_device::{
    actors::wifi::*,
    traits::{ip::*, wifi::*},
    *,
};

use serde::Serialize;
use serde_cbor::ser::SliceWrite;
use serde_cbor::Serializer;

pub struct DrogueApi<'a, A>
where
    A: Adapter + 'static,
{
    ssid: &'static str,
    psk: &'static str,
    ip: IpAddress,
    port: u16,
    adapter: Option<WifiAdapter<'a, A>>,
    socket: Option<Socket<'a, A>>,
}

impl<'a, A> DrogueApi<'a, A>
where
    A: Adapter,
{
    pub fn new(ssid: &'static str, psk: &'static str, ip: IpAddress, port: u16) -> Self {
        Self {
            ssid,
            psk,
            ip,
            port,
            socket: None,
            adapter: None,
        }
    }
}

impl<'a, A> Actor for DrogueApi<'a, A>
where
    A: Adapter + 'static,
{
    type Configuration = WifiAdapter<'a, A>;
    #[rustfmt::skip]
    type Message<'m> where 'a: 'm = Measurement;
    #[rustfmt::skip]
    type OnStartFuture<'m> where 'a: 'm = impl Future<Output = ()> + 'm;
    #[rustfmt::skip]
    type OnMessageFuture<'m> where 'a: 'm = impl Future<Output = ()> + 'm;

    fn on_mount(&mut self, config: Self::Configuration) {
        self.adapter.replace(config);
    }

    fn on_start<'m>(mut self: Pin<&'m mut Self>) -> Self::OnStartFuture<'m> {
        async move {
            log::info!("Joining access point");
            let adapter = self.adapter.take().unwrap();
            adapter
                .join(Join::Wpa {
                    ssid: self.ssid,
                    password: self.psk,
                })
                .await
                .expect("Error joining wifi");
            log::info!("Joined access point");

            let socket = adapter.socket().await;
            self.adapter.replace(adapter);

            log::info!("Connecting to {}:{}", self.ip, self.port);
            let result = socket
                .connect(IpProtocol::Tcp, SocketAddress::new(self.ip, self.port))
                .await;
            match result {
                Ok(_) => {
                    self.socket.replace(socket);
                    log::info!("Connected to {:?}!", self.ip);
                }
                Err(e) => {
                    log::warn!("Error connecting: {:?}", e);
                }
            }
        }
    }

    fn on_message<'m>(
        mut self: Pin<&'m mut Self>,
        message: Self::Message<'m>,
    ) -> Self::OnMessageFuture<'m> {
        async move {
            let socket = self.socket.take().expect("socket not bound!");

            let mut buf = [0; 256];
            let writer = SliceWrite::new(&mut buf[..]);
            let mut ser = Serializer::new(writer);
            match message.serialize(&mut ser) {
                Ok(_) => {
                    let writer = ser.into_inner();
                    let size = writer.bytes_written();
                    let result = socket.send(&buf[..size]).await;
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
            self.socket.replace(socket);
        }
    }
}
