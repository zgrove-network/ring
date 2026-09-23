use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use zcash_keys::keys::UnifiedFullViewingKey;
use zcash_protocol::consensus::Network;

use crate::derive;
use crate::ledger::Ledger;

/// The face the market talks to.
///
/// It can read the book and commit to it. It cannot spend: the spending key
/// is not here, and paying a withdrawal is a separate command run somewhere
/// else. So the worst an intruder does is see who is owed what and move
/// balances between rounds — bad, and not the same as taking the money.
pub struct Server {
    ledger: Mutex<Ledger>,
    ufvk: UnifiedFullViewingKey,
    network: Network,
}

type Shared = Arc<Server>;

/// An error the caller is allowed to read.
struct Refusal(StatusCode, String);

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

fn bad(message: impl std::fmt::Display) -> Refusal {
    Refusal(StatusCode::BAD_REQUEST, message.to_string())
}

/// Reads the bearer token and turns it into the address it speaks for.
///
/// An unknown token is not distinguished from a missing one: saying which is
/// which would let somebody test tokens against this endpoint and learn from
/// the difference.
fn who(server: &Server, headers: &HeaderMap) -> Result<u32, Refusal> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");

    let ledger = server.ledger.lock().expect("the ledger mutex was poisoned");
    match ledger.resolve(token) {
        Ok(Some(index)) => Ok(index),
        _ => Err(Refusal(
            StatusCode::UNAUTHORIZED,
            "that token is not one of ours".into(),
        )),
    }
}

#[derive(Serialize)]
struct Joined {
    index: u32,
    address: String,
    token: String,
    network: &'static str,
    unit: &'static str,
    note: &'static str,
}

async fn join(State(server): State<Shared>) -> Result<Json<Joined>, Refusal> {
    // Thirty-two bytes from the system generator. This is the only thing
    // standing between a stranger and somebody's balance, so it is not a
    // counter, a timestamp, or anything derived from one.
    let mut raw = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();

    // A requested index and the diversifier that actually works are not the
    // same number, and several requests can land on one address. So the
    // address is resolved first and the index it really came out at is what
    // gets claimed; if somebody took it in between, step past it and retry.
    let mut wanted = {
        let ledger = server.ledger.lock().expect("the ledger mutex was poisoned");
        ledger
            .next_index()
            .map_err(|e| Refusal(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    let (index, address) = loop {
        let (address, actual) = derive(&server.ufvk, wanted).map_err(bad)?;
        let taken = {
            let mut ledger = server.ledger.lock().expect("the ledger mutex was poisoned");
            ledger.claim_at(actual, "web", &token, now())
        };
        match taken {
            Ok(()) => break (actual, address),
            Err(_) if actual < u32::MAX => wanted = actual + 1,
            Err(e) => {
                return Err(Refusal(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
            }
        }
    };

    Ok(Json(Joined {
        index,
        address: address.encode(&server.network),
        token,
        network: name_of(&server.network),
        unit: unit_of(&server.network),
        note: "keep this token; it is the only way back to this balance",
    }))
}

#[derive(Serialize)]
struct Standing {
    index: u32,
    address: String,
    available: i64,
    /// Said on every answer, because an address alone does not say it loudly
    /// enough. A page that reads "send ZEC here" beside a testnet address is
    /// an instruction to destroy money.
    network: &'static str,
    /// What the figures on this page are denominated in.
    unit: &'static str,
}

async fn me(State(server): State<Shared>, headers: HeaderMap) -> Result<Json<Standing>, Refusal> {
    let index = who(&server, &headers)?;
    let (address, _) = derive(&server.ufvk, index).map_err(bad)?;
    let available = {
        let ledger = server.ledger.lock().expect("the ledger mutex was poisoned");
        ledger.available(index).map_err(bad)?
    };
    Ok(Json(Standing {
        index,
        address: address.encode(&server.network),
        available,
        network: name_of(&server.network),
        unit: unit_of(&server.network),
    }))
}

#[derive(Deserialize)]
struct Stake {
    settles_on: u32,
    outcome: String,
    zatoshi: u64,
}

async fn bet(
    State(server): State<Shared>,
    headers: HeaderMap,
    Json(stake): Json<Stake>,
) -> Result<Json<Standing>, Refusal> {
    let index = who(&server, &headers)?;
    {
        let mut ledger = server.ledger.lock().expect("the ledger mutex was poisoned");
        let round = ledger.open_round(stake.settles_on).map_err(bad)?;
        ledger
            .place(round, index, &stake.outcome, stake.zatoshi, now())
            .map_err(bad)?;
    }
    me(State(server), headers).await
}

#[derive(Deserialize)]
struct Leaving {
    zatoshi: u64,
    to: String,
}

async fn withdraw(
    State(server): State<Shared>,
    headers: HeaderMap,
    Json(leaving): Json<Leaving>,
) -> Result<Json<Standing>, Refusal> {
    let index = who(&server, &headers)?;
    {
        let mut ledger = server.ledger.lock().expect("the ledger mutex was poisoned");
        ledger
            .request_withdrawal(index, leaving.zatoshi, &leaving.to, now())
            .map_err(bad)?;
    }
    me(State(server), headers).await
}

fn name_of(network: &Network) -> &'static str {
    match network {
        Network::MainNetwork => "mainnet",
        Network::TestNetwork => "testnet",
    }
}

/// Testnet coins are called TAZ and are worth nothing. Calling them ZEC on a
/// page that also prints an address is how somebody sends real money to a
/// testnet address and never sees it again.
fn unit_of(network: &Network) -> &'static str {
    match network {
        Network::MainNetwork => "ZEC",
        Network::TestNetwork => "TAZ",
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub async fn run(
    network: Network,
    ufvk_text: &str,
    data: std::path::PathBuf,
    bind: &str,
) -> Result<()> {
    let ufvk = UnifiedFullViewingKey::decode(&network, ufvk_text)
        .map_err(|e| anyhow!("that is not a viewing key for this network: {e}"))?;
    let ledger = Ledger::open(&data.join("ledger.sqlite"))?;

    let server: Shared = Arc::new(Server {
        ledger: Mutex::new(ledger),
        ufvk,
        network,
    });

    let app = Router::new()
        .route("/v1/join", post(join))
        .route("/v1/me", get(me))
        .route("/v1/bet", post(bet))
        .route("/v1/withdraw", post(withdraw))
        // The market is served from somewhere else entirely — a static host,
        // a different origin — so the browser has to be told this is allowed.
        // Any origin, because there is nothing here a stranger's page can do
        // without a token, and the token is not a cookie the browser would
        // attach on its own.
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(server);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    println!("listening on {bind}");
    axum::serve(listener, app).await.context("serving")?;
    Ok(())
}
