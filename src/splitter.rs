use core::future::Future;

use core::pin::Pin;
use drogue_device::*;

#[rustfmt::skip]
pub struct Splitter<'a, M, A, B>
where
    M: Copy,
    A: Actor<Message<'a> = M> + 'static,
    B: Actor<Message<'a> = M> + 'static,
{
    a: Option<Address<'static, A>>,
    b: Option<Address<'static, B>>,
    _d: core::marker::PhantomData<&'a M>,
}

#[rustfmt::skip]
impl<'a, M, A, B> Splitter<'a, M, A, B>
where
    M: Copy,
    A: Actor<Message<'a> = M> + 'static,
    B: Actor<Message<'a> = M> + 'static,
{
    pub fn new() -> Self {
        Self {
            a: None,
            b: None,
            _d: core::marker::PhantomData,
        }
    }
}

#[rustfmt::skip]
impl<'a, M, A, B> Actor for Splitter<'a, M, A, B>
where
    M: Copy,
    A: Actor<Message<'a> = M> + 'static,
    B: Actor<Message<'a> = M> + 'static,
{
    type Configuration = (Address<'static, A>, Address<'static, B>);

    type Message<'m> where 'a: 'm = M;

    type OnStartFuture<'m> where 'a: 'm, A: 'm, B: 'm, M: 'm = impl Future<Output = ()> + 'm;

    type OnMessageFuture<'m> where 'a: 'm, A: 'm, B: 'm, M: 'm  = impl Future<Output = ()> + 'm;

    fn on_mount(&mut self, config: Self::Configuration) {
        self.a.replace(config.0);
        self.b.replace(config.1);
    }

    fn on_start<'m>(self: Pin<&'m mut Self>) -> Self::OnStartFuture<'m> {
        async move {}
    }

    fn on_message<'m>(
        self: Pin<&'m mut Self>,
        message: Self::Message<'m>,
    ) -> Self::OnMessageFuture<'m> {
        async move {
            if let Some(a) = self.a.as_ref() {
                a.request(message).unwrap().await;
            }
            if let Some(b) = self.b.as_ref() {
                b.request(message).unwrap().await;
            }
        }
    }
}
