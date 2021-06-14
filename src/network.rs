use crate::plant_monitor::Measurement;
use core::{future::Future, marker::PhantomData};

use core::pin::Pin;
use drogue_device::{actors::wifi::*, traits::ip::*, *};

use serde::Serialize;
use serde_json_core::ser::to_slice;

pub struct DrogueApi<'a, A, M>
where
    A: Adapter + 'static,
    M: From<Measurement> + Serialize + 'static,
{
    ip: IpAddress,
    port: u16,
    adapter: Option<WifiAdapter<'a, A>>,
    socket: Option<Socket<'a, A>>,
    _conv: core::marker::PhantomData<M>,
}

impl<'a, A, M> DrogueApi<'a, A, M>
where
    A: Adapter,
    M: From<Measurement> + Serialize + 'static,
{
    pub fn new(ip: IpAddress, port: u16) -> Self {
        Self {
            ip,
            port,
            socket: None,
            adapter: None,
            _conv: PhantomData,
        }
    }
}

impl<'a, A, M> Actor for DrogueApi<'a, A, M>
where
    A: Adapter + 'static,
    M: From<Measurement> + Serialize + 'static,
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

    fn on_start<'m>(self: Pin<&'m mut Self>) -> Self::OnStartFuture<'m> {
        async move {
            let this = unsafe { self.get_unchecked_mut() };
            let adapter = this.adapter.take().unwrap();

            let socket = adapter.socket().await;
            this.adapter.replace(adapter);
            this.socket.replace(socket);
        }
    }

    fn on_message<'m>(
        self: Pin<&'m mut Self>,
        message: Self::Message<'m>,
    ) -> Self::OnMessageFuture<'m> {
        async move {
            let this = unsafe { self.get_unchecked_mut() };
            if let Some(socket) = this.socket.take() {
                log::info!("Connecting to {}:{}", this.ip, this.port);
                let result = socket
                    .connect(IpProtocol::Tcp, SocketAddress::new(this.ip, this.port))
                    .await;
                match result {
                    Ok(_) => {
                        log::info!("Connected to {:?}!", this.ip);
                        let data: M = message.into();

                        let mut buf = [0; 256];
                        match to_slice(&data, &mut buf) {
                            Ok(size) => {
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
                    }
                    Err(e) => {
                        log::warn!("Error connecting: {:?}", e);
                    }
                }
                this.socket.replace(socket);
            } else {
                log::warn!("Socket not bound, skipping sending report");
            }
        }
    }
}
