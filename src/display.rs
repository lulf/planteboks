use super::plant_monitor::Measurement;
use core::future::Future;
use core::pin::Pin;
use drogue_device::{
    actors::{
        led::matrix::{LEDMatrix, MatrixCommand},
        ticker::{Ticker, TickerCommand},
    },
    *,
};
use embassy::time::{Duration, Timer};
use embassy_nrf::gpio::{AnyPin, Output};

type LedMatrix = LEDMatrix<Output<'static, AnyPin>, 5, 5>;

pub struct Display {
    matrix: ActorContext<'static, LedMatrix>,
    refresher: ActorContext<'static, Ticker<'static, LedMatrix>>,
    display: ActorContext<'static, DisplayActor>,
}

impl Display {
    pub fn new(
        pin_rows: [Output<'static, AnyPin>; 5],
        pin_cols: [Output<'static, AnyPin>; 5],
    ) -> Self {
        Self {
            matrix: ActorContext::new(LEDMatrix::new(pin_rows, pin_cols)),
            refresher: ActorContext::new(Ticker::new(
                Duration::from_millis(1000 / 200),
                MatrixCommand::Render,
            )),
            display: ActorContext::new(DisplayActor::new()),
        }
    }
}

impl Package for Display {
    type Primary = DisplayActor;

    type Configuration = ();

    fn mount<S: ActorSpawner>(
        &'static self,
        _: Self::Configuration,
        spawner: S,
    ) -> Address<Self::Primary> {
        let matrix = self.matrix.mount((), spawner);
        let refresher = self.refresher.mount(matrix, spawner);
        self.display.mount((matrix, refresher), spawner)
    }
}

pub struct DisplayActor {
    matrix: Option<Address<'static, LedMatrix>>,
    refresher: Option<Address<'static, Ticker<'static, LedMatrix>>>,
}

impl DisplayActor {
    pub fn new() -> Self {
        Self {
            matrix: None,
            refresher: None,
        }
    }
}

impl Actor for DisplayActor {
    type Configuration = (
        Address<'static, LedMatrix>,
        Address<'static, Ticker<'static, LedMatrix>>,
    );

    type Message<'m> = Measurement;
    type OnStartFuture<'m> = impl Future<Output = ()> + 'm;
    type OnMessageFuture<'m> = impl Future<Output = ()> + 'm;

    fn on_mount(&mut self, _: Address<'static, Self>, config: Self::Configuration) {
        self.matrix.replace(config.0);
        self.refresher.replace(config.1);
    }

    fn on_start<'m>(self: Pin<&'m mut Self>) -> Self::OnStartFuture<'m> {
        async move {
            self.refresher.unwrap().notify(TickerCommand::Stop).unwrap();
        }
    }

    fn on_message<'m>(
        self: Pin<&'m mut Self>,
        message: Self::Message<'m>,
    ) -> Self::OnMessageFuture<'m> {
        async move {
            log::trace!("Displaying measurement");
            self.refresher
                .unwrap()
                .request(TickerCommand::Start)
                .unwrap()
                .await;

            let mut temp: u8 = if message.temperature.is_negative() {
                self.matrix
                    .unwrap()
                    .request(MatrixCommand::ApplyFrame(&'-'))
                    .unwrap()
                    .await;
                Timer::after(Duration::from_secs(1)).await;
                message.temperature.abs() as u8
            } else {
                message.temperature as u8
            };

            while temp != 0 {
                let c = char::from_digit(
                    if temp < 10 {
                        let d = temp;
                        temp = 0;
                        d
                    } else {
                        let d = temp / 10;
                        temp %= 10;
                        d
                    } as u32,
                    10,
                )
                .unwrap();
                self.matrix
                    .unwrap()
                    .request(MatrixCommand::ApplyFrame(&c))
                    .unwrap()
                    .await;

                Timer::after(Duration::from_secs(1)).await;
            }

            self.refresher
                .unwrap()
                .request(TickerCommand::Stop)
                .unwrap()
                .await;

            self.matrix
                .unwrap()
                .request(MatrixCommand::Clear)
                .unwrap()
                .await;
            self.matrix
                .unwrap()
                .request(MatrixCommand::Render)
                .unwrap()
                .await;
        }
    }
}
