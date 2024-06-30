mod utils;

use bytes::Bytes;
use http::header;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::client::conn::http1::Builder;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper::{Method, Request, Response};
use log::info;
use std::collections::{hash_map, HashMap};
use std::error::Error;
use std::net::SocketAddr;
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
    path : String,
    remote_url: String,
}

async fn proxy(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Box<dyn Error + Send + Sync>> {

   let  proxy_configs = HashMap::from([
     (String::from("/archlinux"), ProxyConfig{
        path: String::from("/archlinux"),
        remote_url: String::from("http://mirrors.bfsu.edu.cn/archlinux"),
     }),
     (String::from("/docker"), ProxyConfig{
        path: String::from("/docker"),
        remote_url: String::from("https://registry.docker.com"),
     }),
   ]);

    let uri = req.uri();

   let paths: Vec<&str> =  String::from(uri.path()).splitn(3, "/").collect();

 let match_path=   match paths.get(1) {
    Some(path) =>  String::from(*path),
    None => String::new(),
   };

let proxy_config =   match proxy_configs.get(&match_path) {
    Some(proxy_config) => proxy_config,
    None => todo!(),
  };


    info!("origin uri {}", req.uri());
    let mut uri_str = match uri.host() {
        Some(_) => Result::Ok(uri.to_string()),
        None => match req.headers().get(header::HOST) {
            Some(host) => match host.to_str() {
                Ok(host_str) => Result::Ok(format!("http://{}{}", host_str, req.uri())),
                Err(err) => Result::Err(Box::new(err)),
            },
            None => Result::Ok(String::new()),
        },
    }?;

    let real_uri = match uri.host() {
        Some(_) => {
            return Ok(uri.clone());
        },
        None => {
            match req.headers().get(header::HOST) {
                Some(host) => match host.to_str() {
                    Ok(host_str) => Result::Ok(format!("http://{}{}", host_str, req.uri())),
                    Err(err) => Result::Err(Box::new(err)),
                },
                None => todo!(),
            },
        },
    }?; 

    if uri_str.starts_with("http:////127.0.0.1:8100/archlinux") {
    uri_str = uri_str.replace("127.0.0.1:8100/archlinux", "mirrors.bfsu.edu.cn/archlinux");
    }

    info!("uri_str {}", uri_str);

    let stream = TcpStream::connect("mirrors.bfsu.edu.cn:80").await?;

    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;

    tokio::task::spawn(async move {
        if let Err(err) = conn.await {
            println!("Connection failed: {:?}", err);
        }
    });

    let mut builder = Request::builder()
        .uri(req.uri())
        .method(req.method())
        .version(req.version());

    let req_headers = req.headers();

    for (key, value) in req_headers {
        if key == header::HOST {
            info!("host");
            builder = builder.header(header::HOST, "mirrors.bfsu.edu.cn");
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

fn host_addr(uri: &http::Uri) -> Option<String> {
    uri.authority().and_then(|auth| Some(auth.to_string()))
}

fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

// Create a TCP connection to host:port, build a tunnel between the connection and
// the upgraded connection
async fn tunnel(upgraded: Upgraded, addr: String) -> std::io::Result<()> {
    // Connect to remote server
    let mut server = TcpStream::connect(addr).await?;
    let mut upgraded = TokioIo::new(upgraded);

    // Proxying data
    let (from_client, from_server) =
        tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;

    // Print message when done
    println!(
        "client wrote {} bytes and received {} bytes",
        from_client, from_server
    );

    Ok(())
}
