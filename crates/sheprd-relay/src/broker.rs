//! The steps both transports share, in one place so QUIC and the WebSocket
//! fallback cannot drift apart: admitting a listener, applying what it
//! says about who may dial, and taking a dial from proof to accepted peer.
//! Each step counts its outcome and answers with a wire [`Status`].

use crate::{
    RouteId,
    bucket::Outcome,
    hub::{Accepted, DialRefusal, Hub, ListenRefusal, Registration, Session},
    metrics::{Dial, Listen, Offense},
    route::prove,
    wire::{Key, Message, Status},
};
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::time::timeout;

/// Checks a listen request: the address's rate, the token, and the proof
/// over `signed`; then registers the listener. `token` is empty when none
/// was given.
pub fn listen(
    hub: &Arc<Hub>,
    address: IpAddr,
    token: &[u8],
    key: &[u8],
    signature: &[u8],
    signed: &[u8],
) -> Result<Registration, Status> {
    // Each refusal counts its outcome and, when the caller was at fault,
    // an offense toward a ban.
    let refuse = |status, outcome, offense: Option<Offense>| {
        hub.metrics.listen(outcome);
        if let Some(offense) = offense {
            hub.offend(address, offense);
        }
        status
    };
    if let Some(offense) = limited(hub.allow_listen(address)) {
        return Err(refuse(Status::Limited, Listen::Limited, offense));
    }
    let Some(admitted) = hub.access.admit((!token.is_empty()).then_some(token)) else {
        let offense = Some(Offense::BadProof);
        return Err(refuse(Status::Forbidden, Listen::Forbidden, offense));
    };
    let Ok(route) = prove(key, signature, signed) else {
        let offense = Some(Offense::BadProof);
        return Err(refuse(Status::Refused, Listen::BadProof, offense));
    };
    let registration =
        hub.register_with(route, address, admitted)
            .map_err(|refusal| match refusal {
                ListenRefusal::Banned | ListenRefusal::Full => {
                    refuse(Status::Full, Listen::Full, None)
                }
                ListenRefusal::Revoked => refuse(Status::Forbidden, Listen::Forbidden, None),
                ListenRefusal::TooMany => {
                    refuse(Status::TooMany, Listen::Full, Some(Offense::Limited))
                }
            })?;
    hub.metrics.listen(Listen::Ready);
    Ok(registration)
}

/// What a rate limit's `outcome` costs the caller: nothing when allowed
/// (`None`); otherwise the refusal, with an offense only when the caller
/// went too fast, not when the limiter was full of others.
fn limited(outcome: Outcome) -> Option<Option<Offense>> {
    match outcome {
        Outcome::Allowed => None,
        Outcome::Limited => Some(Some(Offense::Limited)),
        Outcome::Full => Some(None),
    }
}

/// Applies what a listener says about who may dial it, unless it says too
/// much too fast.
pub fn apply(hub: &Arc<Hub>, registration: &Registration, message: Message) -> Status {
    if !registration.allow_message() {
        hub.offend(registration.address, Offense::Limited);
        return Status::Limited;
    }
    match message {
        Message::DialKeys(keys) => {
            registration.set_dial_keys(&keys);
            Status::Ok
        }
        Message::PairingKey { key, seconds } => {
            let lifetime = Duration::from_secs(u64::from(seconds));
            if registration.add_pairing_key(key, lifetime) {
                Status::Ok
            } else {
                Status::TooMany
            }
        }
    }
}

/// Takes a dial from its proof to an open session: checks the address's
/// rate and the proof over `signed`, waits for the listener, and opens the
/// session if the key is still allowed. A key the listener does not allow,
/// or revoked while the dial waited, answers `Offline`, exactly as no
/// listener would.
pub async fn dial(
    hub: &Arc<Hub>,
    address: IpAddr,
    route: RouteId,
    key: &Key,
    signature: &[u8],
    signed: &[u8],
) -> Result<(Accepted, Session), Status> {
    let refuse = |status, outcome, offense: Option<Offense>| {
        hub.metrics.dial(outcome);
        if let Some(offense) = offense {
            hub.offend(address, offense);
        }
        status
    };
    if let Some(offense) = limited(hub.allow_dial(address)) {
        return Err(refuse(Status::Limited, Dial::Limited, offense));
    }
    if prove(key, signature, signed).is_err() {
        let offense = Some(Offense::BadProof);
        return Err(refuse(Status::Offline, Dial::Unauthorized, offense));
    }
    let waiting = match hub.dial(route, *key, address) {
        Ok(Ok(waiting)) => waiting,
        // Neither an empty route nor a key the listener did not name costs
        // a point: they must cost the same, as they answer the same, so a
        // caller's ban cannot reveal whether a computer is online; and a
        // phone retrying while its computer is off must not get its
        // household banned. The dial rate limit catches a flood of either.
        Ok(Err(DialRefusal::Offline)) => {
            return Err(refuse(Status::Offline, Dial::Offline, None));
        }
        Ok(Err(DialRefusal::Unauthorized)) => {
            return Err(refuse(Status::Offline, Dial::Unauthorized, None));
        }
        Ok(Err(DialRefusal::Busy)) | Err(_) => {
            return Err(refuse(Status::Busy, Dial::Busy, None));
        }
    };
    let accepted = match timeout(hub.limits.accept_timeout, waiting.accepted).await {
        Ok(Ok(accepted)) => accepted,
        Ok(Err(_)) | Err(_) => return Err(refuse(Status::Timeout, Dial::Timeout, None)),
    };
    drop(waiting.pending);
    let Some(session) = hub.open(waiting.slot) else {
        // Revoked while it waited: the device may not know yet.
        return Err(refuse(Status::Offline, Dial::Unauthorized, None));
    };
    hub.metrics.dial(Dial::Spliced);
    Ok((accepted, session))
}
