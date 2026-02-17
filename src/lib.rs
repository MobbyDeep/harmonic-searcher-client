use std::sync::Arc;
use std::time::{Duration, SystemTime};

use prost_types::Timestamp;
use thiserror::Error;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use tonic::Request;

pub mod auth {
    tonic::include_proto!("auth");
}

pub mod bundle {
    tonic::include_proto!("bundle");
}

pub mod packet {
    tonic::include_proto!("packet");
}

pub mod searcher {
    tonic::include_proto!("searcher");
}

pub mod shared {
    tonic::include_proto!("shared");
}

use auth::auth_service_client::AuthServiceClient;
use auth::{
    GenerateAuthChallengeRequest, GenerateAuthTokensRequest, RefreshAccessTokenRequest, Role,
};
use searcher::searcher_service_client::SearcherServiceClient;
use searcher::SendBundleRequest;

pub type SignerFn = dyn Fn(&[u8]) -> Result<Vec<u8>, HarmonicSearcherError> + Send + Sync;

#[derive(Debug, Error)]
pub enum HarmonicSearcherError {
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(#[from] tonic::codegen::http::uri::InvalidUri),
    #[error("transport error: {0}")]
    Transport(#[from] tonic::transport::Error),
    #[error("rpc status: {0}")]
    Status(#[from] tonic::Status),
    #[error("signer failed: {0}")]
    Signer(String),
    #[error("missing access token in auth response")]
    MissingAccessToken,
    #[error("missing refresh token in auth response")]
    MissingRefreshToken,
    #[error("invalid auth token expiry timestamp")]
    InvalidExpiry,
    #[error("invalid bearer token metadata")]
    InvalidBearerToken,
}

#[derive(Clone, Debug)]
struct TokenState {
    access_token: String,
    access_token_expires_at: SystemTime,
    refresh_token: String,
}

pub struct HarmonicSearcherClient {
    auth_client: AuthServiceClient<Channel>,
    searcher_client: SearcherServiceClient<Channel>,
    pubkey: [u8; 32],
    signer: Arc<SignerFn>,
    token_state: TokenState,
    refresh_before_expiry: Duration,
}

impl HarmonicSearcherClient {
    pub async fn connect(
        endpoint: impl Into<String>,
        pubkey: [u8; 32],
        signer: Arc<SignerFn>,
    ) -> Result<Self, HarmonicSearcherError> {
        let endpoint = Endpoint::from_shared(endpoint.into())?;
        let channel = endpoint.connect().await?;
        let auth_client = AuthServiceClient::new(channel.clone());
        let searcher_client = SearcherServiceClient::new(channel);

        let mut client = Self {
            auth_client,
            searcher_client,
            pubkey,
            signer,
            token_state: TokenState {
                access_token: String::new(),
                access_token_expires_at: SystemTime::UNIX_EPOCH,
                refresh_token: String::new(),
            },
            refresh_before_expiry: Duration::from_secs(15),
        };

        client.authenticate().await?;
        Ok(client)
    }

    pub fn with_refresh_skew(mut self, refresh_before_expiry: Duration) -> Self {
        self.refresh_before_expiry = refresh_before_expiry;
        self
    }

    pub async fn authenticate(&mut self) -> Result<(), HarmonicSearcherError> {
        let challenge_response = self
            .auth_client
            .generate_auth_challenge(GenerateAuthChallengeRequest {
                role: Role::Searcher as i32,
                pubkey: self.pubkey.to_vec(),
            })
            .await?
            .into_inner();

        let mut sign_payload = self.pubkey.to_vec();
        sign_payload.extend_from_slice(challenge_response.challenge.as_bytes());
        let signed_challenge = (self.signer)(&sign_payload)?;

        let tokens = self
            .auth_client
            .generate_auth_tokens(GenerateAuthTokensRequest {
                challenge: challenge_response.challenge,
                client_pubkey: self.pubkey.to_vec(),
                signed_challenge,
            })
            .await?
            .into_inner();

        let access = tokens
            .access_token
            .ok_or(HarmonicSearcherError::MissingAccessToken)?;
        let refresh = tokens
            .refresh_token
            .ok_or(HarmonicSearcherError::MissingRefreshToken)?;

        self.token_state = TokenState {
            access_token: access.value,
            access_token_expires_at: timestamp_to_system_time(access.expires_at_utc)?,
            refresh_token: refresh.value,
        };

        Ok(())
    }

    pub async fn refresh_access_token(&mut self) -> Result<(), HarmonicSearcherError> {
        let refreshed = self
            .auth_client
            .refresh_access_token(RefreshAccessTokenRequest {
                refresh_token: self.token_state.refresh_token.clone(),
            })
            .await?
            .into_inner();

        let access = refreshed
            .access_token
            .ok_or(HarmonicSearcherError::MissingAccessToken)?;

        self.token_state.access_token = access.value;
        self.token_state.access_token_expires_at = timestamp_to_system_time(access.expires_at_utc)?;
        Ok(())
    }

    pub async fn send_bundle(
        &mut self,
        bundle: bundle::Bundle,
    ) -> Result<String, HarmonicSearcherError> {
        self.ensure_fresh_access_token().await?;

        let mut request = Request::new(SendBundleRequest {
            bundle: Some(bundle),
        });

        let bearer = format!("Bearer {}", self.token_state.access_token);
        let metadata_token = MetadataValue::try_from(bearer.as_str())
            .map_err(|_| HarmonicSearcherError::InvalidBearerToken)?;
        request
            .metadata_mut()
            .insert("authorization", metadata_token);

        let response = self
            .searcher_client
            .send_bundle(request)
            .await?
            .into_inner();
        Ok(response.uuid)
    }

    pub fn access_token_expires_at(&self) -> SystemTime {
        self.token_state.access_token_expires_at
    }

    async fn ensure_fresh_access_token(&mut self) -> Result<(), HarmonicSearcherError> {
        let now = SystemTime::now();
        let refresh_deadline = now + self.refresh_before_expiry;
        if refresh_deadline >= self.token_state.access_token_expires_at {
            self.refresh_access_token().await?;
        }
        Ok(())
    }
}

pub fn bundle_from_raw_transactions(
    transactions: impl IntoIterator<Item = Vec<u8>>,
) -> bundle::Bundle {
    let packets = transactions
        .into_iter()
        .map(|tx| packet::Packet {
            data: tx.clone(),
            meta: Some(packet::Meta {
                size: tx.len() as u64,
                addr: String::new(),
                port: 0,
                flags: Some(packet::PacketFlags::default()),
                sender_stake: 0,
            }),
        })
        .collect();

    bundle::Bundle {
        header: Some(shared::Header {
            ts: Some(Timestamp::from(SystemTime::now())),
        }),
        packets,
    }
}

fn timestamp_to_system_time(ts: Option<Timestamp>) -> Result<SystemTime, HarmonicSearcherError> {
    let ts = ts.ok_or(HarmonicSearcherError::InvalidExpiry)?;
    ts.try_into()
        .map_err(|_| HarmonicSearcherError::InvalidExpiry)
}
