use capnp;
use capnp::serialize_packed;
use crypto::identity::PublicKey;
use common::int_convert::usize_to_u32;
use crate::capnp_common::{write_signature, read_signature,
                          write_custom_int128, read_custom_int128,
                          write_custom_u_int128, read_custom_u_int128,
                          write_rand_nonce, read_rand_nonce,
                          write_uid, read_uid,
                          write_invoice_id, read_invoice_id,
                          write_public_key, read_public_key,
                          write_relay_address, read_relay_address,
                          write_index_server_address, read_index_server_address,
                          write_receipt, read_receipt};

use app_server_capnp;
use crate::serialize::SerializeError;

use crate::index_server::messages::IndexServerAddress;
use crate::funder::messages::{RelayAddress, UserRequestSendFunds, 
    ResponseReceived, ResponseSendFundsResult};
use crate::funder::serialize::{ser_friends_route, deser_friends_route};

use super::messages::{AppServerToApp, AppToAppServer, 
                        NodeReport, NodeReportMutation};


fn ser_user_request_send_funds(user_request_send_funds: &UserRequestSendFunds,
                          user_request_send_funds_builder: &mut app_server_capnp::user_request_send_funds::Builder) {

    write_uid(&user_request_send_funds.request_id, 
              &mut user_request_send_funds_builder.reborrow().init_request_id());

    let mut route_builder = user_request_send_funds_builder.reborrow().init_route();
    ser_friends_route(&user_request_send_funds.route, &mut route_builder);

    write_custom_u_int128(user_request_send_funds.dest_payment, 
              &mut user_request_send_funds_builder.reborrow().init_dest_payment());

    write_invoice_id(&user_request_send_funds.invoice_id, 
              &mut user_request_send_funds_builder.reborrow().init_invoice_id());
}

fn deser_user_request_send_funds(user_request_send_funds_reader: &app_server_capnp::user_request_send_funds::Reader)
    -> Result<UserRequestSendFunds, SerializeError> {

    Ok(UserRequestSendFunds {
        request_id: read_uid(&user_request_send_funds_reader.get_request_id()?)?,
        route: deser_friends_route(&user_request_send_funds_reader.get_route()?)?,
        dest_payment: read_custom_u_int128(&user_request_send_funds_reader.get_dest_payment()?)?,
        invoice_id: read_invoice_id(&user_request_send_funds_reader.get_invoice_id()?)?,
    })
}

fn ser_response_received(response_received: &ResponseReceived,
                          response_received_builder: &mut app_server_capnp::response_received::Builder) {

    write_uid(&response_received.request_id, 
              &mut response_received_builder.reborrow().init_request_id());

    let mut result_builder = response_received_builder.reborrow().init_result();
    match &response_received.result {
        ResponseSendFundsResult::Success(receipt) => {
            let mut success_builder = result_builder.init_success();
            write_receipt(receipt, &mut success_builder);
        },
        ResponseSendFundsResult::Failure(public_key) => {
            let mut failure_builder = result_builder.init_failure();
            write_public_key(public_key, &mut failure_builder);
        },
    };
}

fn deser_response_received(response_received_reader: &app_server_capnp::response_received::Reader)
    -> Result<ResponseReceived, SerializeError> {

    let result = match response_received_reader.get_result().which()? {
        app_server_capnp::response_received::result::Success(receipt_reader) => {
            let receipt_reader = receipt_reader?;
            ResponseSendFundsResult::Success(read_receipt(&receipt_reader)?)
        },
        app_server_capnp::response_received::result::Failure(public_key_reader) => {
            let public_key_reader = public_key_reader?;
            ResponseSendFundsResult::Failure(read_public_key(&public_key_reader)?)
        },
    };

    Ok(ResponseReceived {
        request_id: read_uid(&response_received_reader.get_request_id()?)?,
        result,
    })
}


// ---------------------------------------------------
// ---------------------------------------------------

pub fn serialize_app_server_to_app(app_server_to_app: &AppServerToApp<RelayAddress, IndexServerAddress>) -> Vec<u8> {
    // TODO
    unimplemented!();
}

pub fn deserialize_app_server_to_app(data: &[u8]) -> Result<AppServerToApp<RelayAddress, IndexServerAddress>, SerializeError> {
    // TODO
    unimplemented!();
}

pub fn serialize_app_to_app_server(app_server_to_app: &AppToAppServer<RelayAddress, IndexServerAddress>) -> Vec<u8> {
    // TODO
    unimplemented!();
}

pub fn deserialize_app_to_app_server(data: &[u8]) -> Result<AppToAppServer<RelayAddress, IndexServerAddress>, SerializeError> {
    // TODO
    unimplemented!();
}
