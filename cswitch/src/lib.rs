#![feature(conservative_impl_trait)]
#![allow(unused)]

#[macro_use]
extern crate log;
extern crate ring;
extern crate rand;
extern crate bytes;
extern crate capnp;
extern crate futures;
extern crate byteorder;
extern crate tokio_core;
extern crate tokio_io;

pub mod crypto;

pub mod inner_messages;
mod networker_state_machine;
// mod prefix_frame_codec;

mod close_handle;
pub mod security_module;
pub mod channeler;

pub mod async_mutex;
mod service_client;
pub mod timer;

mod schema;
use schema::channeler_capnp;
