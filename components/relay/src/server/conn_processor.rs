use std::marker::Unpin;

use futures::{future, FutureExt, SinkExt, Stream, StreamExt};

use common::conn::{ConnPair, ConnPairVec, SinkError};

use timer::utils::future_timeout;
use timer::TimerClient;

use super::types::{
    IncomingAccept, IncomingConn, IncomingConnInner, IncomingConnect, IncomingListen,
};

use proto::crypto::PublicKey;
use proto::proto_ser::{ProtoDeserialize, ProtoSerialize};
use proto::relay::messages::{IncomingConnection, InitConnection, RejectConnection};

async fn dispatch_conn(
    conn_pair_vec: ConnPairVec,
    public_key: PublicKey,
    first_msg: Vec<u8>,
) -> Option<IncomingConn> {
    let (sender, receiver) = conn_pair_vec.split();

    let sender = sender.sink_map_err(|_| ());
    let inner = match InitConnection::proto_deserialize(&first_msg).ok()? {
        InitConnection::Listen => {
            let conn_pair = ConnPair::from_raw(
                sender
                    .sink_map_err(|_| SinkError)
                    .with(|msg: IncomingConnection| {
                        future::ready::<Result<_, SinkError>>(Ok(msg.proto_serialize()))
                    }),
                receiver
                    .map(|data| RejectConnection::proto_deserialize(&data))
                    .take_while(|res| future::ready(res.is_ok()))
                    .map(Result::unwrap),
            );
            IncomingConnInner::Listen(IncomingListen { conn_pair })
        }
        InitConnection::Accept(accept_public_key) => IncomingConnInner::Accept(IncomingAccept {
            accept_public_key,
            conn_pair: ConnPairVec::from_raw(sender, receiver),
        }),
        InitConnection::Connect(connect_public_key) => {
            IncomingConnInner::Connect(IncomingConnect {
                connect_public_key,
                conn_pair: ConnPairVec::from_raw(sender, receiver),
            })
        }
    };

    Some(IncomingConn { public_key, inner })
}

async fn process_conn(
    mut conn_pair_vec: ConnPairVec,
    public_key: PublicKey,
    mut timer_client: TimerClient,
    conn_timeout_ticks: usize,
) -> Option<IncomingConn> {
    let fut_receiver = Box::pin(async move {
        if let Some(first_msg) = conn_pair_vec.receiver.next().await {
            // Added boxed because of issue: https://github.com/rust-lang/rust/issues/64496#issuecomment-546874018
            // We might be able to remove this later
            let dispatch_res = dispatch_conn(conn_pair_vec, public_key, first_msg)
                .boxed()
                .await;
            if dispatch_res.is_none() {
                warn!("process_conn(): dispatch_conn() failure");
            }
            dispatch_res
        } else {
            None
        }
    });

    let timer_stream = timer_client.request_timer_stream().await.unwrap();
    let res = future_timeout(fut_receiver, timer_stream, conn_timeout_ticks).await?;
    if res.is_none() {
        warn!("process_conn(): timeout occurred");
    }
    res
}

/// Process incoming connections
/// For each connection obtain the first message, and prepare the correct type according to this
/// first messages.
/// If waiting for the first message takes too long, discard the connection.
pub fn conn_processor<T>(
    incoming_conns: T,
    timer_client: TimerClient,
    conn_timeout_ticks: usize,
) -> impl Stream<Item = IncomingConn>
where
    T: Stream<Item = (PublicKey, ConnPairVec)> + Unpin + Send + 'static,
{
    incoming_conns
        .map(move |(public_key, conn_pair_vec)| {
            process_conn(
                conn_pair_vec,
                public_key,
                timer_client.clone(),
                conn_timeout_ticks,
            )
        })
        .filter_map(|opt_conn| opt_conn)
        // Added boxed because of issue: https://github.com/rust-lang/rust/issues/64496#issuecomment-546874018
        // We might be able to remove this later
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::channel::mpsc;
    use futures::executor::{LocalPool, ThreadPool};
    use futures::task::{Spawn, SpawnExt};
    use futures::{stream, FutureExt};

    use common::async_test_utils::receive;
    use proto::crypto::PublicKey;
    use timer::create_timer_incoming;

    async fn task_dispatch_conn_basic(spawner: impl Spawn + Clone) {
        // Create a mock time service:
        let (_tick_sender, tick_receiver) = mpsc::channel::<()>(0);
        let _timer_client = create_timer_incoming(tick_receiver, spawner.clone()).unwrap();

        let (sender, receiver) = mpsc::channel::<Vec<u8>>(0);
        let first_msg = InitConnection::Listen;
        let ser_first_msg = first_msg.proto_serialize();
        let public_key = PublicKey::from(&[0x77; PublicKey::len()]);
        let incoming_conn = dispatch_conn(
            ConnPairVec::from_raw(sender, receiver),
            public_key.clone(),
            ser_first_msg,
        )
        .await
        .unwrap();

        assert_eq!(incoming_conn.public_key, public_key);
        match incoming_conn.inner {
            IncomingConnInner::Listen(_incoming_listen) => {}
            _ => panic!("Wrong IncomingConnInner"),
        };

        let (sender, receiver) = mpsc::channel::<Vec<u8>>(0);
        let accept_public_key = PublicKey::from(&[0x22; PublicKey::len()]);
        let first_msg = InitConnection::Accept(accept_public_key.clone());
        let ser_first_msg = first_msg.proto_serialize();
        let public_key = PublicKey::from(&[0x77; PublicKey::len()]);
        let incoming_conn = dispatch_conn(
            ConnPairVec::from_raw(sender, receiver),
            public_key.clone(),
            ser_first_msg,
        )
        .await
        .unwrap();

        assert_eq!(incoming_conn.public_key, public_key);
        match incoming_conn.inner {
            IncomingConnInner::Accept(incoming_accept) => {
                assert_eq!(incoming_accept.accept_public_key, accept_public_key)
            }
            _ => panic!("Wrong IncomingConnInner"),
        };

        let (sender, receiver) = mpsc::channel::<Vec<u8>>(0);
        let connect_public_key = PublicKey::from(&[0x33; PublicKey::len()]);
        let first_msg = InitConnection::Connect(connect_public_key.clone());
        let ser_first_msg = first_msg.proto_serialize();
        let public_key = PublicKey::from(&[0x77; PublicKey::len()]);
        let incoming_conn = dispatch_conn(
            ConnPairVec::from_raw(sender, receiver),
            public_key.clone(),
            ser_first_msg,
        )
        .await
        .unwrap();

        assert_eq!(incoming_conn.public_key, public_key);
        match incoming_conn.inner {
            IncomingConnInner::Connect(incoming_connect) => {
                assert_eq!(incoming_connect.connect_public_key, connect_public_key)
            }
            _ => panic!("Wrong IncomingConnInner"),
        };
    }

    #[test]
    fn test_dispatch_conn_basic() {
        let thread_pool = ThreadPool::new().unwrap();
        LocalPool::new().run_until(task_dispatch_conn_basic(thread_pool.clone()));
    }

    async fn task_dispatch_conn_invalid_first_msg(spawner: impl Spawn + Clone) {
        // Create a mock time service:
        let (_tick_sender, tick_receiver) = mpsc::channel::<()>(0);
        let _timer_client = create_timer_incoming(tick_receiver, spawner.clone()).unwrap();

        let (sender, receiver) = mpsc::channel::<Vec<u8>>(0);
        let ser_first_msg = b"This is an invalid message".to_vec();
        let public_key = PublicKey::from(&[0x77; PublicKey::len()]);
        let res = dispatch_conn(
            ConnPairVec::from_raw(sender, receiver),
            public_key.clone(),
            ser_first_msg,
        )
        .await;
        assert!(res.is_none());
    }

    #[test]
    fn test_dispatch_conn_invalid_first_msg() {
        let thread_pool = ThreadPool::new().unwrap();
        LocalPool::new().run_until(task_dispatch_conn_invalid_first_msg(thread_pool.clone()));
    }

    #[test]
    fn test_conn_processor_basic() {
        let thread_pool = ThreadPool::new().unwrap();

        // Create a mock time service:
        let (_tick_sender, tick_receiver) = mpsc::channel::<()>(0);
        let timer_client = create_timer_incoming(tick_receiver, thread_pool.clone()).unwrap();

        let public_key = PublicKey::from(&[0x77; PublicKey::len()]);
        let (local_sender, _remote_receiver) = mpsc::channel::<Vec<u8>>(0);
        let (mut remote_sender, local_receiver) = mpsc::channel::<Vec<u8>>(0);

        let incoming_conns = stream::iter::<_>(vec![(
            public_key.clone(),
            ConnPairVec::from_raw(local_sender, local_receiver),
        )]);

        let conn_timeout_ticks = 16;

        let processed_conns =
            conn_processor(incoming_conns, timer_client, conn_timeout_ticks).boxed();

        let processed_conns = Box::pin(processed_conns);

        let first_msg = InitConnection::Listen;
        let ser_first_msg = first_msg.proto_serialize();
        thread_pool
            .spawn(async move {
                remote_sender
                    .send(ser_first_msg)
                    .map(|res| match res {
                        Ok(_remote_sender) => (),
                        Err(_) => unreachable!("Sending first message failed!"),
                    })
                    .await
            })
            .unwrap();

        let (conn, processed_conns) = LocalPool::new()
            .run_until(receive(processed_conns))
            .unwrap();
        assert_eq!(conn.public_key, public_key);
        match conn.inner {
            IncomingConnInner::Listen(_incoming_listen) => {}
            _ => panic!("Incorrect processed conn"),
        };

        assert!(LocalPool::new()
            .run_until(receive(processed_conns))
            .is_none());
    }
}
