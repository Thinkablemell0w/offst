#![feature(futures_api, async_await, await_macro, arbitrary_self_types)]
#![feature(nll)]
#![feature(try_from)]
#![feature(generators)]
#![feature(never_type)]

#![deny(
    trivial_numeric_casts,
    warnings
)]

// #[macro_use]
// extern crate log;

mod setup_conn;
pub mod identity;
pub mod connect;
pub mod proto;
