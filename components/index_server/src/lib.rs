#![crate_type = "lib"] 
#![feature(futures_api, async_await, await_macro, arbitrary_self_types)]
#![feature(nll)]
#![feature(try_from)]
#![feature(generators)]
#![feature(never_type)]
#![feature(map_get_key_value)]

#![deny(
    trivial_numeric_casts,
    warnings
)]

#[macro_use]
extern crate log;

#[macro_use]
extern crate common;

mod graph;
mod verifier;
mod backoff_connector;
mod server;
mod net_server;

pub use net_server::{net_index_server, NetIndexServerError};
