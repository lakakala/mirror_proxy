mod error;
mod utils;

use bytes::Bytes;
use error::Result;
use http::{header, uri::Uri};
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{server::conn::http1, service::service_fn, Request, Response};
use log::info;
use std::{collections::HashMap, error::Error, net::SocketAddr};
use tokio::net::{TcpListener, TcpStream};
use utils::tokio_adapter::TokioIo;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::SimpleLogger::new().init().unwrap();

    let addr = SocketAddr::from(([127, 0, 0, 1], 8100));

    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);

    loop {
        let (stream, remote_addr) = listener.accept().await?;

        info!("accept conn {}", remote_addr.to_string());
        let io = utils::tokio_adapter::TokioIo::new(stream);

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
    let proxy_configs = HashMap::from([
        (
            String::from("archlinux"),
            ProxyConfig {
                path: String::from("/archlinux"),
                redirect_url: Uri::from_static("http://mirrors.bfsu.edu.cn/archlinux"),
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

    let remote_path = match paths.get(3) {
        Some(path) => {
            format!("{}/{}", proxy_config.redirect_url.path(), path)
        }
        None => String::from(proxy_config.redirect_url.path()),
    };

    info!("remote_path {}", remote_path);

    let stream = TcpStream::connect(format!(
        "{}:{}",
        proxy_config.redirect_url.host().unwrap(),
        proxy_config.redirect_url.port().unwrap()
    ))
    .await?;

    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;

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
            info!("host");
            builder = builder.header(header::HOST, proxy_config.redirect_url.host().unwrap());
        } else {
            builder = builder.header(key, value);
        }
    }

    let proxy_req = builder.body(full(req.collect().await?.to_bytes()))?;

    let proxy_resp = sender.send_request(proxy_req).await?;

    let mut resp_builder = Response::builder()
        .status(proxy_resp.status())
        .version(proxy_resp.version());

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
