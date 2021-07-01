use crate::plant_monitor::Measurement;
use core::{future::Future, marker::PhantomData};

use core::pin::Pin;
use drogue_device::{
    clients::http::*,
    traits::{ip::*, tcp::*},
    *,
};

use serde::Serialize;
use serde_json_core::ser::to_slice;

pub struct NetworkEndpoint<A, M>
where
    A: TcpSocket + 'static,
    M: From<Measurement> + Serialize + 'static,
{
    ip: IpAddress,
    port: u16,
    username: &'static str,
    password: &'static str,
    socket: Option<A>,
    _conv: core::marker::PhantomData<M>,
}

impl<A, M> NetworkEndpoint<A, M>
where
    A: TcpSocket + 'static,
    M: From<Measurement> + Serialize + 'static,
{
    pub fn new(ip: IpAddress, port: u16, username: &'static str, password: &'static str) -> Self {
        Self {
            ip,
            port,
            username,
            password,
            socket: None,
            _conv: PhantomData,
        }
    }
}

impl<A, M> Actor for NetworkEndpoint<A, M>
where
    A: TcpSocket + 'static,
    M: From<Measurement> + Serialize + 'static,
{
    type Configuration = A;

    type Message<'m> = Measurement;
    type OnStartFuture<'m> = impl Future<Output = ()> + 'm;
    type OnMessageFuture<'m> = impl Future<Output = ()> + 'm;

    fn on_mount(&mut self, _: Address<'static, Self>, config: Self::Configuration) {
        self.socket.replace(config);
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
            if let Some(mut socket) = this.socket.take() {
                let data: M = message.into();
                let mut buf = [0; 256];
                match to_slice(&data, &mut buf) {
                    Ok(size) => {
                        let mut client = HttpClient::new(
                            &mut socket,
                            this.ip,
                            this.port,
                            this.username,
                            this.password,
                        );
                        let mut rx_buf = [0; 32];
                        let result = client
                            .post(
                                "/v1/foo?data_schema=urn:no:lulf:plantmonitor",
                                &buf[..size],
                                "application/json",
                                &mut rx_buf[..],
                            )
                            .await;
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
                this.socket.replace(socket);
            } else {
                log::warn!("Socket not bound, skipping sending report");
            }
        }
    }
}
