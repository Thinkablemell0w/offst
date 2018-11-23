@0xe7603b9ac00e2251;

using import "common.capnp".Signature;
using import "common.capnp".PublicKey;
using import "common.capnp".RandNonce;
using import "common.capnp".InvoiceId;
using import "common.capnp".Uid;
using import "common.capnp".CustomUInt128;
using import "common.capnp".CustomInt128;


# Token channel messages
# ----------------------

struct MoveToken {
        operations @0: List(FriendOperation);
        # Ordered batched operations for this move token.
        # First operation should be applied first.
        oldToken @1: Signature;
        # Token of the previous move token. This is a proof that we have
        # received the previous message before sending this one.
        inconsistencyCounter @2: UInt64;
        # Amount of inconsistencies that have occured so far. Begins from 0,
        # and increases every time an inconsistency was resolved.
        moveTokenCounter @3: CustomUInt128;
        # Amount of MoveToken messages in this token channel. Begins from 0,
        # and increases every time a MoveToken message increases. This number
        # is shared for both sides of the token channel.
        balance @4 : CustomInt128;
        # Balance between the two parties in the token channel. This number could be 
        # deduced by each of the parties only by looking at the operations
        # field. We put the balance here to make sure it is signed by the
        # sending party, allowing the receiving party to use
        # this MoveToken as a proof later.
        # Note that the balance here is represented from the point of view of the sender.
        # The receiver will have to negate this value.
        localPendingDebt @5: CustomUInt128;
        # The current local pending debt from the point of view of the sender.
        remotePendingDebt @6: CustomUInt128;
        # The current remote pending debt from the point of view of the sender.
        randNonce @7: RandNonce;
        # A random nonce, generated by the sender. We have it because the
        # sender is signing over this message, and we don't want him to be
        # tricked into signing over something strange.
        newToken @8 : Signature;
        # A signature over all the previous fields.
}

struct MoveTokenRequest {
        moveToken @0: MoveToken;
        tokenWanted @1: Bool;
}

struct InconsistencyError {
        resetToken @0: Signature;
        inconsistencyCounter @1: UInt64;
        balanceForReset @2: CustomInt128;
}


# A messages sent between friends.
struct FriendMessage {
        union {
                moveTokenRequest @0: MoveTokenRequest;
                inconsistencyError @1: InconsistencyError;
        }
}




# Token Operations
# ------------------

# Set the maximum possible debt for the remote party.
# Note: It is not possible to set a maximum debt smaller than the current debt
# This will cause an inconsistency.
# struct SetRemoteMaxDebtOp {
#         remoteMaxDebt @0: CustomUInt128;
# }

struct FriendsRoute {
        nodePublicKeys @0: List(PublicKey);
        # A list of public keys
}

# A custom type for a rational 128 bit number.
struct Ratio128 {
        union {
                one @0: Void;
                numerator @1: CustomUInt128;
        }
}

struct FreezeLink {
        sharedCredits @0: CustomUInt128;
        # Credits shared for freezing through previous edge.
        usableRatio @1: Ratio128;
        # Ratio of credits that can be used for freezing from the previous
        # edge. Ratio might only be an approximation to real value, if the real
        # value can not be represented as a u128/u128.
}


struct RequestSendFundsOp { 
        requestId @0: Uid;
        route @1: FriendsRoute;
        destPayment @2: CustomUInt128;
        invoiceId @3: InvoiceId;
        freezeLinks @4: List(FreezeLink);
        # Variable amount of freezing links. This is used for protection
        # against DoS of credit freezing by have exponential decay of available
        # credits freezing according to derived trust.
        # This part should not be signed in the Response message.
}

struct ResponseSendFundsOp {
        requestId @0: Uid;
        randNonce @1: RandNonce;
        signature @2: Signature;
        # Signature{key=recipientKey}(
        #   sha512/256("FUND_SUCCESS") ||
        #   sha512/256(requestId || sha512/256(route) || randNonce) ||
        #   destPayment ||
        #   invoiceId
        # )
        #
        # Note that the signature contains an inner blob (requestId || ...).
        # This is done to make the size of the receipt shorter.
        # See also the Receipt structure.
}

struct FailureSendFundsOp {
        requestId @0: Uid;
        reportingPublicKey @1: PublicKey;
        # Index of the reporting node in the route of the corresponding request.
        # The reporting node cannot be the destination node.
        randNonce @2: RandNonce;
        signature @3: Signature;
        # Signature{key=recipientKey}(
        #   sha512/256("FUND_FAILURE") ||
        #   requestId ||
        #   sha512/256(route) || 
        #   destPayment ||
        #   invoiceId ||
        #   reportingPublicKey ||
        #   randNonce
        # )
}


struct FriendOperation {
        union {
                enableRequests @0: Void;
                disableRequests @1: Void;
                setRemoteMaxDebt @2: CustomUInt128;
                requestSendFunds @3: RequestSendFundsOp;
                responseSendFunds @4: ResponseSendFundsOp;
                failureSendFunds @5: FailureSendFundsOp;
        }
}

