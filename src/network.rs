use crate::{http, plant_monitor::Measurement};
use core::{future::Future, marker::PhantomData};

use core::pin::Pin;
use drogue_device::{actors::wifi::*, traits::ip::*, *};

use serde::Serialize;
use serde_json_core::ser::to_slice;

pub struct NetworkEndpoint<'a, A, M>
where
    A: Adapter + 'static,
    M: From<Measurement> + Serialize + 'static,
{
    ip: IpAddress,
    port: u16,
    username: &'static str,
    password: &'static str,
    adapter: Option<WifiAdapter<'a, A>>,
    _conv: core::marker::PhantomData<M>,
}

impl<'a, A, M> NetworkEndpoint<'a, A, M>
where
    A: Adapter,
    M: From<Measurement> + Serialize + 'static,
{
    pub fn new(ip: IpAddress, port: u16, username: &'static str, password: &'static str) -> Self {
        Self {
            ip,
            port,
            username,
            password,
            adapter: None,
            _conv: PhantomData,
        }
    }
}

impl<'a, A, M> Actor for NetworkEndpoint<'a, A, M>
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

    fn on_mount(&mut self, _: Address<'static, Self>, config: Self::Configuration) {
        self.adapter.replace(config);
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
            if let Some(adapter) = this.adapter.take() {
                let data: M = message.into();
                let mut buf = [0; 256];
                match to_slice(&data, &mut buf) {
                    Ok(size) => {
                        let socket = adapter.socket().await;
                        let mut client = http::HttpClient::new(
                            socket,
                            this.ip,
                            this.port,
                            this.username,
                            this.password,
                        );
                        let result = client
                            .post("/v1/foo?data_schema=urn:no:lulf:plantmonitor", &buf[..size])
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
                this.adapter.replace(adapter);
            } else {
                log::warn!("Adapter not bound, skipping sending report");
            }
        }
    }
}
