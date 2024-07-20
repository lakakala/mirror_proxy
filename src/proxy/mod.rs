use hyper::{Request, Response, body::Incoming};

struct ProxyNamespace{
    namespace: String,
}
