mod error;
mod proxy;
mod utils;

use bytes::Bytes;
use error::Result;
use futures_util::TryStreamExt;
use http::{header, uri::Uri};
use http_body::Frame;
use http_body_util::{combinators::BoxBody, BodyExt, StreamBody};
use hyper::{server::conn::http1, service::service_fn, Request, Response};
use hyper_util::rt::TokioIo;
use log::info;
use native_tls::TlsConnector;
use snafu::ResultExt;
use std::{collections::HashMap, error::Error, net::SocketAddr};
use tokio::net::{TcpListener, TcpStream};
use utils::proxy_adapter;
use utils::proxy_stream::ProxyStream;
use utils::tls_adapter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::SimpleLogger::new().init().unwrap();

    let addr = SocketAddr::from(([0, 0, 0, 0], 8100));

    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);

    loop {
        let (stream, remote_addr) = listener.accept().await?;

        info!("accept conn {}", remote_addr.to_string());
        let io = TokioIo::new(stream);

        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .serve_connection(io, service_fn(proxy))
                .await
            {
                println!("Failed to serve connection: {:?}", err);
            }
        });
    }
}

struct ProxyConfig {
    path: String,
    redirect_url: Uri,
    proxy: Option<String>,
}

async fn proxy(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Box<dyn Error + Send + Sync>> {
    info!("origin_url {}", req.uri().to_string());
    let proxy_configs = HashMap::from([
        (
            String::from("bfsu"),
            ProxyConfig {
                path: String::from("/archlinux"),
                redirect_url: Uri::from_static("https://mirrors.bfsu.edu.cn"),
                proxy: Option::Some(String::from("192.168.124.200:7890")),
            },
        ),
        (
            String::from("docker"),
            ProxyConfig {
                path: String::from("/docker"),
                redirect_url: Uri::from_static("https://05gomin9.mirror.aliyuncs.com"),
                proxy: Option::Some(String::from("192.168.124.200:7890")),
            },
        ),
    ]);

    let uri = req.uri();

    let host = match uri.host() {
        Some(host) => Ok(host),
        None => match req.headers().get(header::HOST) {
            Some(raw_host) => match raw_host.to_str() {
                Ok(host) => Ok(host),
                Err(_) => Err(error::Error::NoFoundNamespace),
            },
            None => Err(error::Error::NoFoundNamespace),
        },
    }?;

    let labels = host.splitn(2, ".").collect::<Vec<&str>>();
    if labels.len() != 2 {
        return Err(error::Error::NoFoundNamespace).boxed();
    }

    let namespace = match labels.get(0) {
        Some(path) => Ok(String::from(*path)),
        None => Err(error::Error::NoFoundNamespace),
    }?;

    let proxy_config = match proxy_configs.get(&namespace) {
        Some(proxy_config) => Ok(proxy_config),
        None => Err(error::Error::UnknownNamespace {
            namespace: namespace,
        }),
    }?;

    let remote_host = match proxy_config.redirect_url.host() {
        Some(host) => Ok(host),
        None => Err(error::Error::IllegalRedireactUrl {
            redirect_url: proxy_config.redirect_url.to_string(),
        }),
    }?;

    let remote_port: u16 = match proxy_config.redirect_url.port() {
        Some(p) => Ok(p.as_u16()),
        None => match proxy_config.redirect_url.scheme() {
            Some(scheme) => {
                if scheme == "http" {
                    Ok(80)
                } else if scheme == "https" {
                    Ok(443)
                } else {
                    Err(error::Error::IllegalRedireactUrl {
                        redirect_url: proxy_config.redirect_url.to_string(),
                    })
                }
            }
            None => Err(error::Error::IllegalRedireactUrl {
                redirect_url: proxy_config.redirect_url.to_string(),
            }),
        },
    }?;

    let remote_addr = format!("{}:{}", remote_host, remote_port);

    let path = match uri.path_and_query() {
        Some(path) => path.as_str(),
        None => "/",
    };

    info!(
        "local_host {} remote_host {} path {}",
        host, remote_addr, path
    );

    let stream = match &proxy_config.proxy {
        Some(proxy_addr) => proxy_adapter::MaybeProxyStream::proxy_stream(
            ProxyStream::connect(proxy_addr.clone(), remote_addr).await?,
        ),
        None => proxy_adapter::MaybeProxyStream::tcp_stream(TcpStream::connect(remote_addr).await?),
    };

    let remote_stream = match proxy_config.redirect_url.scheme() {
        Some(scheme) => {
            if scheme == "http" {
                Ok(tls_adapter::MaybeHttpsStream::new_http(TokioIo::new(
                    stream,
                )))
            } else if scheme == "https" {
                let cx = TlsConnector::builder().build()?;
                let cx = tokio_native_tls::TlsConnector::from(cx);

                let stream = cx.connect(remote_host, stream).await?;

                Ok(tls_adapter::MaybeHttpsStream::new_https(TokioIo::new(
                    stream,
                )))
            } else {
                Err(error::Error::IllegalRedireactUrl {
                    redirect_url: proxy_config.redirect_url.to_string(),
                })
            }
        }
        None => Err(error::Error::IllegalRedireactUrl {
            redirect_url: proxy_config.redirect_url.to_string(),
        }),
    }?;

    let (mut sender, conn) = hyper::client::conn::http1::handshake(remote_stream).await?;

    tokio::task::spawn(async move {
        if let Err(err) = conn.await {
            println!("Connection failed: {:?}", err);
        }
    });

    let mut builder = Request::builder()
        .uri(path)
        .method(req.method())
        .version(req.version());

    let req_headers = req.headers();

    for (key, value) in req_headers {
        if key == header::HOST {
            info!("host {}", remote_host);
            builder = builder.header(header::HOST, remote_host);
        } else {
            builder = builder.header(key, value);
        }
    }

    //    let box_body =  BoxBody::new(a);
    let proxy_req = builder.body(Box::new(
        StreamBody::new(req.into_data_stream().map_ok(Frame::data)).boxed(),
    ))?;

    let proxy_resp = sender.send_request(proxy_req).await?;

    let mut resp_builder = Response::builder()
        .status(proxy_resp.status())
        .version(proxy_resp.version());

    info!("proxy_resp status_code {}", proxy_resp.status());

    for (key, value) in proxy_resp.headers() {
        resp_builder = resp_builder.header(key, value);
    }

    let resp = resp_builder
        .body(StreamBody::new(proxy_resp.into_data_stream().map_ok(Frame::data)).boxed())?;

    return Ok(resp);
}
