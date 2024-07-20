use std::pin::Pin;

use super::proxy_stream;
use tokio::net::TcpStream;

pub enum MaybeProxyStream {
    ProxyStream(proxy_stream::ProxyStream),
    TcpStream(TcpStream),
}

impl MaybeProxyStream {
    pub fn proxy_stream(proxy_stream: proxy_stream::ProxyStream) -> MaybeProxyStream {
        MaybeProxyStream::proxy_stream(proxy_stream)
    }

    pub fn tcp_stream(tcp_stream: TcpStream) -> MaybeProxyStream {
        MaybeProxyStream::TcpStream(tcp_stream)
    }
}

impl tokio::io::AsyncRead for MaybeProxyStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match Pin::get_mut(self) {
            MaybeProxyStream::ProxyStream(proxy_stream) => {
                Pin::new(proxy_stream).poll_read(cx, buf)
            }
            MaybeProxyStream::TcpStream(tcp_stream) => Pin::new(tcp_stream).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for MaybeProxyStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        match Pin::get_mut(self) {
            MaybeProxyStream::ProxyStream(proxy_stream) => {
                Pin::new(proxy_stream).poll_write(cx, buf)
            }
            MaybeProxyStream::TcpStream(tcp_stream) => Pin::new(tcp_stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match Pin::get_mut(self) {
            MaybeProxyStream::ProxyStream(proxy_stream) => Pin::new(proxy_stream).poll_flush(cx),
            MaybeProxyStream::TcpStream(tcp_stream) => Pin::new(tcp_stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match Pin::get_mut(self) {
            MaybeProxyStream::ProxyStream(proxy_stream) => Pin::new(proxy_stream).poll_shutdown(cx),
            MaybeProxyStream::TcpStream(tcp_stream) => Pin::new(tcp_stream).poll_shutdown(cx),
        }
    }
}
