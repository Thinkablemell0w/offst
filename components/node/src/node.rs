use futures::task::{Spawn, SpawnExt};
use futures::{Future, FutureExt, Stream, StreamExt, SinkExt};
use futures::channel::mpsc;

use common::conn::{ConnPairVec, FutTransform};
use crypto::crypto_rand::CryptoRandom;
use crypto::identity::PublicKey;

use database::DatabaseClient;
use identity::IdentityClient;
use timer::TimerClient;

use app_server::{IncomingAppConnection, app_server_loop};
use channeler::{spawn_channeler, ChannelerError};
use keepalive::KeepAliveChannel;
use secure_channel::SecureChannel;
use funder::{funder_loop, FunderError, FunderState};
use funder::types::{FunderIncomingComm, FunderOutgoingComm, 
    IncomingLivenessMessage, ChannelerConfig};

use index_client::{IndexClientError, spawn_index_client};

use proto::funder::messages::{RelayAddress, TcpAddress, 
    FunderToChanneler, ChannelerToFunder, 
    FunderIncomingControl, FunderOutgoingControl};
use proto::funder::serialize::{serialize_friend_message, 
    deserialize_friend_message};
use proto::funder::report::funder_report_to_index_client_state;
use proto::index_server::messages::IndexServerAddress;
use proto::index_client::messages::{AppServerToIndexClient, IndexClientToAppServer};

use crate::types::{NodeMutation, NodeState, create_node_report, NodeConfig};
use crate::adapters::{EncRelayConnector, EncKeepaliveConnector};

#[derive(Debug)]
pub enum NodeError {
    RequestPublicKeyError,
    SpawnError,
}


fn node_spawn_channeler<C,R,S>(node_config: &NodeConfig,
                          local_public_key: PublicKey,
                          identity_client: IdentityClient,
                          timer_client: TimerClient,
                          net_connector: C,
                          rng: R,
                          from_funder: mpsc::Receiver<FunderToChanneler<Vec<RelayAddress>>>,
                          to_funder: mpsc::Sender<ChannelerToFunder>,
                          spawner: S) 
    -> Result<impl Future<Output=Result<(), ChannelerError>>, NodeError>

where
    C: FutTransform<Input=TcpAddress,Output=Option<ConnPairVec>> + Clone + Send + Sync + 'static,
    R: CryptoRandom + Clone + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    let encrypt_transform = SecureChannel::new(
        identity_client.clone(),
        rng.clone(),
        timer_client.clone(),
        node_config.ticks_to_rekey,
        spawner.clone());

    let keepalive_transform = KeepAliveChannel::new(
        timer_client.clone(),
        node_config.keepalive_ticks,
        spawner.clone());

    let enc_relay_connector = EncRelayConnector::new(encrypt_transform.clone(), net_connector);

    spawn_channeler(local_public_key,
                          timer_client,
                          node_config.backoff_ticks,
                          node_config.conn_timeout_ticks,
                          node_config.max_concurrent_encrypt,
                          enc_relay_connector,
                          encrypt_transform,
                          keepalive_transform,
                          from_funder,
                          to_funder,
                          spawner)
        .map_err(|_| NodeError::SpawnError)
}

fn node_spawn_funder<R,S>(node_config: &NodeConfig,
                identity_client: IdentityClient,
                funder_state: FunderState<Vec<RelayAddress>>,
                mut database_client: DatabaseClient<NodeMutation<RelayAddress,IndexServerAddress>>,
                mut from_channeler: mpsc::Receiver<ChannelerToFunder>,
                mut to_channeler: mpsc::Sender<FunderToChanneler<Vec<RelayAddress>>>,
                from_app_server: mpsc::Receiver<FunderIncomingControl<Vec<RelayAddress>>>,
                to_app_server: mpsc::Sender<FunderOutgoingControl<Vec<RelayAddress>>>,
                rng: R,
                mut spawner: S)
        -> Result<impl Future<Output=Result<(), FunderError>>, NodeError>
where
    R: CryptoRandom + Clone + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{

    // TODO: Should we give a length > 0 for this adapter's channel?
    let (request_sender, mut request_receiver) = mpsc::channel(0);
    let funder_db_client = DatabaseClient::new(request_sender);

    let database_adapter_fut = async move {
        while let Some(request) = await!(request_receiver.next()) {
            let mutations = request.mutations
                .into_iter()
                .map(|funder_mutation| 
                     NodeMutation::Funder(funder_mutation))
                .collect::<Vec<_>>();

            if let Err(_) = await!(database_client.mutate(mutations)) {
                return;
            }
        }
    };
    spawner.spawn(database_adapter_fut)
        .map_err(|_| NodeError::SpawnError)?;



    // Channeler to funder adapter:
    let (mut incoming_comm_sender, incoming_comm) = mpsc::channel(0);
    let channeler_to_funder_adapter = async move {
        while let Some(channeler_message) = await!(from_channeler.next()) {
            let opt_to_funder_message = match channeler_message {
                ChannelerToFunder::Online(public_key) => 
                    Some(FunderIncomingComm::Liveness(IncomingLivenessMessage::Online(public_key))),
                ChannelerToFunder::Offline(public_key) => 
                    Some(FunderIncomingComm::Liveness(IncomingLivenessMessage::Offline(public_key))),
                ChannelerToFunder::Message((public_key, data)) => {
                    if let Ok(friend_message) = deserialize_friend_message(&data[..]) {
                        Some(FunderIncomingComm::Friend((public_key, friend_message)))
                    } else {
                        // We discard the message if we can't deserialize it:
                        None
                    }
                },
            };
            if let Some(to_funder_message) = opt_to_funder_message {
                if let Err(_) = await!(incoming_comm_sender.send(to_funder_message)) {
                    return;
                }
            }
        }
    };

    spawner.spawn(channeler_to_funder_adapter)
        .map_err(|_| NodeError::SpawnError)?;

    let (outgoing_comm_sender, mut outgoing_comm) = mpsc::channel(0);

    // Funder to Channeler adapter:
    let funder_to_channeler_adapter = async move {
        while let Some(funder_message) = await!(outgoing_comm.next()) {
            let to_channeler_message = match funder_message {
                FunderOutgoingComm::ChannelerConfig(channeler_config) =>
                    match channeler_config {
                        ChannelerConfig::SetAddress(relay_addresses) =>
                            FunderToChanneler::SetAddress(relay_addresses),
                        ChannelerConfig::UpdateFriend(channeler_update_friend) =>
                            FunderToChanneler::UpdateFriend(channeler_update_friend),
                        ChannelerConfig::RemoveFriend(friend_public_key) => 
                            FunderToChanneler::RemoveFriend(friend_public_key),
                    },
                FunderOutgoingComm::FriendMessage((public_key, friend_message)) => {
                    let data = serialize_friend_message(&friend_message);
                    FunderToChanneler::Message((public_key, data))
                },
            };
            if let Err(_) = await!(to_channeler.send(to_channeler_message)) {
                return;
            }
        }
    };

    spawner.spawn(funder_to_channeler_adapter)
        .map_err(|_| NodeError::SpawnError)?;

    let funder_fut = funder_loop(
        identity_client.clone(),
        rng.clone(),
        from_app_server,
        incoming_comm,
        to_app_server,
        outgoing_comm_sender,
        node_config.max_operations_in_batch,
        node_config.max_pending_user_requests,
        funder_state,
        funder_db_client);

    spawner.spawn_with_handle(funder_fut)
        .map_err(|_| NodeError::SpawnError)
}

async fn node_spawn_index_client<'a, C,R,S>(node_config: &'a NodeConfig,
                local_public_key: PublicKey,
                identity_client: IdentityClient,
                timer_client: TimerClient,
                node_state: &'a NodeState<RelayAddress, IndexServerAddress>,
                mut database_client: DatabaseClient<NodeMutation<RelayAddress,IndexServerAddress>>,
                from_app_server: mpsc::Receiver<AppServerToIndexClient<IndexServerAddress>>,
                to_app_server: mpsc::Sender<IndexClientToAppServer<IndexServerAddress>>,
                net_connector: C,
                rng: R,
                mut spawner: S)
        -> Result<impl Future<Output=Result<(), IndexClientError>>, NodeError>
where
    C: FutTransform<Input=TcpAddress,Output=Option<ConnPairVec>> + Clone + Send + Sync + 'static,
    R: CryptoRandom + Clone + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{

    let initial_node_report = create_node_report(&node_state);

    // Database adapter:
    let (request_sender, mut request_receiver) = mpsc::channel(0);
    let index_client_db_client = DatabaseClient::new(request_sender);

    let database_adapter_fut = async move {
        while let Some(request) = await!(request_receiver.next()) {
            let mutations = request.mutations
                .into_iter()
                .map(|index_client_mutation| 
                     NodeMutation::IndexClient(index_client_mutation))
                .collect::<Vec<_>>();

            if let Err(_) = await!(database_client.mutate(mutations)) {
                return;
            }
        }
    };
    spawner.spawn(database_adapter_fut)
        .map_err(|_| NodeError::SpawnError)?;


    let index_client_state = funder_report_to_index_client_state(&initial_node_report.funder_report);

    let encrypt_transform = SecureChannel::new(
        identity_client.clone(),
        rng.clone(),
        timer_client.clone(),
        node_config.ticks_to_rekey,
        spawner.clone());

    let keepalive_transform = KeepAliveChannel::new(
        timer_client.clone(),
        node_config.keepalive_ticks,
        spawner.clone());

    let enc_keepalive_connector = EncKeepaliveConnector::new(
        encrypt_transform.clone(),
        keepalive_transform.clone(),
        net_connector.clone(),
        spawner.clone());

    await!(spawn_index_client(local_public_key,
                node_state.index_client_config.clone(),
                index_client_state,
                identity_client,
                timer_client,
                index_client_db_client,
                from_app_server,
                to_app_server,
                node_config.max_open_index_client_requests,
                node_config.keepalive_ticks,
                node_config.backoff_ticks,
                enc_keepalive_connector,
                rng,
                spawner.clone()))
        .map_err(|_| NodeError::SpawnError)

}

#[allow(unused)]
pub async fn spawn_node<C,IA,R,S>(
                node_config: NodeConfig,
                identity_client: IdentityClient,
                timer_client: TimerClient,
                node_state: NodeState<RelayAddress, IndexServerAddress>,
                database_client: DatabaseClient<NodeMutation<RelayAddress,IndexServerAddress>>,
                net_connector: C,
                incoming_apps: IA,
                rng: R,
                mut spawner: S) -> Result<impl Future, NodeError> 
where
    C: FutTransform<Input=TcpAddress,Output=Option<ConnPairVec>> + Clone + Send + Sync + 'static,
    IA: Stream<Item=IncomingAppConnection<RelayAddress,IndexServerAddress>> + Unpin + Send + 'static,  
    R: CryptoRandom + Clone + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    // Get local public key:
    let local_public_key = await!(identity_client.request_public_key())
        .map_err(|_| NodeError::RequestPublicKeyError)?;

    let initial_node_report = create_node_report(&node_state);

    // Channeler <--> Funder
    let (channeler_to_funder_sender, channeler_to_funder_receiver) = mpsc::channel(node_config.channel_len);
    let (funder_to_channeler_sender, funder_to_channeler_receiver) = mpsc::channel(node_config.channel_len);

    let channeler_handle = node_spawn_channeler(&node_config,
                    local_public_key.clone(),
                    identity_client.clone(),
                    timer_client.clone(),
                    net_connector.clone(),
                    rng.clone(),
                    funder_to_channeler_receiver,
                    channeler_to_funder_sender,
                    spawner.clone())?;

    // AppServer <--> Funder
    let (app_server_to_funder_sender, app_server_to_funder_receiver) = mpsc::channel(node_config.channel_len);
    let (funder_to_app_server_sender, funder_to_app_server_receiver) = mpsc::channel(node_config.channel_len);

    let funder_handle = node_spawn_funder(&node_config,
                identity_client.clone(),
                node_state.funder_state.clone(),
                database_client.clone(),
                channeler_to_funder_receiver,
                funder_to_channeler_sender,
                app_server_to_funder_receiver,
                funder_to_app_server_sender,
                rng.clone(),
                spawner.clone())?;

    // AppServer <--> IndexClient
    let (app_server_to_index_client_sender, app_server_to_index_client_receiver) = mpsc::channel(node_config.channel_len);
    let (index_client_to_app_server_sender, index_client_to_app_server_receiver) = mpsc::channel(node_config.channel_len);

    let app_server_fut = app_server_loop(
                funder_to_app_server_receiver,
                app_server_to_funder_sender,
                index_client_to_app_server_receiver,
                app_server_to_index_client_sender,
                incoming_apps,
                initial_node_report.clone(),
                spawner.clone());

    let app_server_handle = spawner.spawn_with_handle(app_server_fut)
        .map_err(|_| NodeError::SpawnError)?;


    let index_client_handle = await!(node_spawn_index_client(
                &node_config,
                local_public_key,
                identity_client,
                timer_client,
                &node_state,
                database_client,
                app_server_to_index_client_receiver,
                index_client_to_app_server_sender,
                net_connector,
                rng,
                spawner))?;

    // Returns a future that resolves after all components were closed:
    Ok(channeler_handle
        .join(funder_handle)
        .join(app_server_handle)
        .join(index_client_handle))
}