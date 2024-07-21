use crate::error::Result;
use bytes::Bytes;
use http::header::HOST;
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use pin_project_lite::pin_project;
use tokio::net::TcpStream;

pin_project! {
    pub struct ProxyStream {
        #[pin]
        upgraded: hyper::upgrade::Upgraded
    }
}

impl ProxyStream {
    pub async fn connect(proxy_addr: String, addr: String) -> Result<ProxyStream> {
        let proxy_req = Request::builder()
            .uri(format!("http://{}", addr))
            .method(http::Method::CONNECT)
            .body(http_body_util::Empty::<Bytes>::new())
            .unwrap();

        let proxy_stream = TcpStream::connect(proxy_addr).await.unwrap();

        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(proxy_stream))
            .await
            .unwrap();

        tokio::task::spawn(async move {
            // Don't forget to enable upgrades on the connection.
            if let Err(err) = conn.with_upgrades().await {
                println!("Connection failed: {:?}", err);
            }
        });

        let res = sender.send_request(proxy_req).await.unwrap();

        let upgraded = hyper::upgrade::on(res).await.unwrap();

        Ok(ProxyStream { upgraded })
    }
}

impl hyper::rt::Read for ProxyStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: hyper::rt::ReadBufCursor<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        self.project().upgraded.poll_read(cx, buf)
    }
}

impl hyper::rt::Write for ProxyStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::result::Result<usize, std::io::Error>> {
        self.project().upgraded.poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        self.project().upgraded.poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        self.project().upgraded.poll_shutdown(cx)
    }
}

impl tokio::io::AsyncRead for ProxyStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        tbuf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        //let init = tbuf.initialized().len();
        let filled = tbuf.filled().len();
        let sub_filled = unsafe {
            let mut buf = hyper::rt::ReadBuf::uninit(tbuf.unfilled_mut());

            match hyper::rt::Read::poll_read(self.project().upgraded, cx, buf.unfilled()) {
                std::task::Poll::Ready(Ok(())) => buf.filled().len(),
                other => return other,
            }
        };

        let n_filled = filled + sub_filled;
        // At least sub_filled bytes had to have been initialized.
        let n_init = sub_filled;
        unsafe {
            tbuf.assume_init(n_init);
            tbuf.set_filled(n_filled);
        }

        std::task::Poll::Ready(Ok(()))
    }
}

impl tokio::io::AsyncWrite for ProxyStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        hyper::rt::Write::poll_write(self.project().upgraded, cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        hyper::rt::Write::poll_flush(self.project().upgraded, cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        hyper::rt::Write::poll_shutdown(self.project().upgraded, cx)
    }

    fn is_write_vectored(&self) -> bool {
        hyper::rt::Write::is_write_vectored(&self.upgraded)
    }

    fn poll_write_vectored(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        hyper::rt::Write::poll_write_vectored(self.project().upgraded, cx, bufs)
    }
}
