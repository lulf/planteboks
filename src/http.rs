use core::fmt::Write;
use drogue_device::{
    actors::wifi::{Adapter, Socket},
    traits::ip::{IpAddress, IpProtocol, SocketAddress},
};
use heapless::{consts, String};

pub struct HttpClient<'a, A>
where
    A: Adapter + 'static,
{
    socket: Socket<'a, A>,
    ip: IpAddress,
    port: u16,
    username: &'a str,
    password: &'a str,
}

impl<'a, A> HttpClient<'a, A>
where
    A: Adapter + 'static,
{
    pub fn new(
        socket: Socket<'a, A>,
        ip: IpAddress,
        port: u16,
        username: &'a str,
        password: &'a str,
    ) -> Self {
        Self {
            socket,
            ip,
            port,
            username,
            password,
        }
    }

    pub async fn post(&mut self, path: &str, payload: &[u8]) -> Result<(), ()> {
        log::info!("Connecting to {}:{}", self.ip, self.port);
        let result = self
            .socket
            .connect(IpProtocol::Tcp, SocketAddress::new(self.ip, self.port))
            .await;

        match result {
            Ok(_) => {
                let mut combined: String<consts::U128> = String::new();
                write!(combined, "{}:{}", self.username, self.password).unwrap();
                let mut authz = [0; 256];
                let authz_len =
                    base64::encode_config_slice(combined.as_bytes(), base64::STANDARD, &mut authz);
                let mut request: String<consts::U512> = String::new();
                write!(request, "POST {} HTTP/1.1\r\n", path).unwrap();
                write!(request, "Authorization: Basic {}\r\n", unsafe {
                    core::str::from_utf8_unchecked(&authz[..authz_len])
                })
                .unwrap();
                write!(request, "Content-Type: application/json\r\n").unwrap();
                write!(request, "Content-Length: {}\r\n\r\n", payload.len()).unwrap();

                log::info!(
                    "Connected to {:?}. Sending request header of {} bytes",
                    self.ip,
                    request.len()
                );
                let result = self.socket.send(&request.as_bytes()[..request.len()]).await;
                match result {
                    Ok(_) => {
                        log::info!("Header sent. Sending payload of {} bytes", payload.len());
                        let result = self.socket.send(payload).await;
                        match result {
                            Ok(_) => {
                                log::info!("Measurement reported");
                            }
                            Err(e) => {
                                log::warn!("Error reporting measurement: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("Error sending headers: {:?}", e);
                    }
                }
            }
            Err(e) => {
                log::warn!("Error connecting: {:?}", e);
            }
        }
        self.socket.close().await;
        Ok(())
    }
}
