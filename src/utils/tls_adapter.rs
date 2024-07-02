use hyper_util::rt::TokioIo;
use std::pin::Pin;
use tokio_native_tls::TlsStream;
pub enum MaybeHttpsStream<T> {
    Http(TokioIo<T>),
    Https(TokioIo<TlsStream<T>>),
}

impl<T> MaybeHttpsStream<T> {
    pub fn new_http(stream: TokioIo<T>) -> MaybeHttpsStream<T> {
        MaybeHttpsStream::Http(stream)
    }

    pub fn new_https(stream: TokioIo<TlsStream<T>>) -> MaybeHttpsStream<T> {
        MaybeHttpsStream::Https(stream)
    }
}

impl<T> hyper::rt::Read for MaybeHttpsStream<T>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: hyper::rt::ReadBufCursor<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match Pin::get_mut(self) {
            MaybeHttpsStream::Http(s) => Pin::new(s).poll_read(cx, buf),
            MaybeHttpsStream::Https(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl<T> hyper::rt::Write for MaybeHttpsStream<T>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        match Pin::get_mut(self) {
            MaybeHttpsStream::Http(s) => Pin::new(s).poll_write(cx, buf),
            MaybeHttpsStream::Https(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match Pin::get_mut(self) {
            MaybeHttpsStream::Http(s) => Pin::new(s).poll_flush(cx),
            MaybeHttpsStream::Https(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match Pin::get_mut(self) {
            MaybeHttpsStream::Http(s) => Pin::new(s).poll_flush(cx),
            MaybeHttpsStream::Https(s) => Pin::new(s).poll_flush(cx),
        }
    }
}
