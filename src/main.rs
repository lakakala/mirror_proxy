mod error;
mod utils;

use bytes::Bytes;
use error::Result;
use http::{header, uri::Uri};
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{server::conn::http1, service::service_fn, Request, Response};
use hyper_util::rt::TokioIo;
use log::info;
use native_tls::TlsConnector;
use std::{collections::HashMap, error::Error, net::SocketAddr};
use tokio::net::{TcpListener, TcpStream};
use utils::tls_adapter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::SimpleLogger::new().init().unwrap();

    let addr = SocketAddr::from(([127, 0, 0, 1], 8100));

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
}

async fn proxy(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Box<dyn Error + Send + Sync>> {
    info!("origin_url {}", req.uri().to_string());
    let proxy_configs = HashMap::from([
        (
            String::from("archlinux"),
            ProxyConfig {
                path: String::from("/archlinux"),
                redirect_url: Uri::from_static("https://mirrors.bfsu.edu.cn/archlinux"),
            },
        ),
        (
            String::from("docker"),
            ProxyConfig {
                path: String::from("/docker"),
                redirect_url: Uri::from_static("https://registry.docker.com"),
            },
        ),
    ]);

    let uri = req.uri();
    let paths: Vec<&str> = match uri.path_and_query() {
        Some(path_and_query) => Ok(path_and_query.as_str().splitn(3, "/").collect()),
        None => Err(error::Error::NoFoundNamespace),
    }?;

    let namespace = match paths.get(1) {
        Some(path) => Ok(String::from(*path)),
        None => Err(error::Error::NoFoundNamespace),
    }?;

    let proxy_config = match proxy_configs.get(&namespace) {
        Some(proxy_config) => Ok(proxy_config),
        None => Err(error::Error::UnknownNamespace {
            namespace: namespace,
        }),
    }?;

    let remote_path = match paths.get(2) {
        Some(path) => {
            format!("{}/{}", proxy_config.redirect_url.path(), path)
        }
        None => String::from(proxy_config.redirect_url.path()),
    };

    info!("remote_path {}", remote_path);

    let host = match proxy_config.redirect_url.host() {
        Some(host) => Ok(host),
        None => Err(error::Error::IllegalRedireactUrl {
            redirect_url: proxy_config.redirect_url.to_string(),
        }),
    }?;

    let port: u16 = match proxy_config.redirect_url.port() {
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

    let remote_addr = format!("{}:{}", host, port);

    info!("remote_addr {}", remote_addr);

    let stream = TcpStream::connect(remote_addr).await?;

    let http_stream = match proxy_config.redirect_url.scheme() {
        Some(scheme) => {
            if scheme == "http" {
                Ok(tls_adapter::MaybeHttpsStream::new_http(TokioIo::new(
                    stream,
                )))
            } else if scheme == "https" {
                let cx = TlsConnector::builder().build()?;
                let cx = tokio_native_tls::TlsConnector::from(cx);

                let mut stream = cx.connect(host, stream).await?;

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

    let (mut sender, conn) = hyper::client::conn::http1::handshake(http_stream).await?;

    tokio::task::spawn(async move {
        if let Err(err) = conn.await {
            println!("Connection failed: {:?}", err);
        }
    });

    let mut builder = Request::builder()
        .uri(remote_path)
        .method(req.method())
        .version(req.version());

    let req_headers = req.headers();

    for (key, value) in req_headers {
        if key == header::HOST {
            info!("host {}", host);
            builder = builder.header(header::HOST, host);
        } else {
            builder = builder.header(key, value);
        }
    }

    let proxy_req = builder.body(full(req.collect().await?.to_bytes()))?;

    let proxy_resp = sender.send_request(proxy_req).await?;

    let mut resp_builder = Response::builder()
        .status(proxy_resp.status())
        .version(proxy_resp.version());

    info!("proxy_resp status_code {}", proxy_resp.status());

    for (key, value) in proxy_resp.headers() {
        resp_builder = resp_builder.header(key, value);
    }

    let b = proxy_resp.collect().await?.to_bytes();
    let resp = resp_builder.body(full(b))?;

    return Ok(resp);
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}
