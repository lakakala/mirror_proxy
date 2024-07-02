use snafu::prelude::*;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("no found namespace"))]
    NoFoundNamespace,
    #[snafu(display("unknown namespace {}", namespace))]
    UnknownNamespace { namespace: String },
    #[snafu(display("illegal redirect url {}", redirect_url))]
    IllegalRedireactUrl{redirect_url: String},
}
