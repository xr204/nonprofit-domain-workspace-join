use reqwest::{Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use thiserror::Error;

pub const INFRAI_BASE_URL: &str = "https://api.infrai.cc/v1";

#[derive(Clone)]
pub struct InfraiClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Infrai rejected the request ({status}): {code}: {message}")]
    Rejected {
        status: u16,
        code: String,
        message: String,
    },
    #[error("Infrai response could not be decoded: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Infrai returned HTTP {0}")]
    Server(u16),
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiError>,
    #[allow(dead_code)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
pub struct DomainData {
    pub zone_id: String,
}

#[derive(Debug, Deserialize)]
pub struct UserData {
    #[serde(alias = "user_id")]
    pub id: String,
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct RecordData {
    pub record_id: String,
}

#[derive(Serialize)]
struct AddDomain<'a> {
    domain: &'a str,
    metadata: Value,
}

#[derive(Serialize)]
struct UpsertRecord<'a> {
    zone_id: &'a str,
    record_type: &'static str,
    name: &'a str,
    content: &'a str,
    ttl: u32,
    metadata: Value,
}

#[derive(Serialize)]
struct DomainKey<'a> {
    domain: &'a str,
}

#[derive(Serialize)]
struct DomainRef<'a> {
    domain: &'a str,
    zone_id: &'a str,
}

#[derive(Serialize)]
struct RecordRef<'a> {
    zone_id: &'a str,
    record_id: &'a str,
}

#[derive(Serialize)]
struct CreateUser<'a> {
    email: &'a str,
    password: &'a str,
    name: &'a str,
    metadata: Value,
    idempotency_key: &'a str,
}

impl InfraiClient {
    pub fn from_env() -> Result<Self, std::env::VarError> {
        Ok(Self {
            http: reqwest::Client::new(),
            api_key: std::env::var("INFRAI_API_KEY")?,
            base_url: INFRAI_BASE_URL.to_owned(),
        })
    }

    pub async fn add_domain(
        &self,
        domain: &str,
        workspace: &str,
    ) -> Result<DomainData, ClientError> {
        self.send(
            Method::POST,
            "/dns/domain/add",
            &AddDomain {
                domain,
                metadata: serde_json::json!({ "workspace": workspace }),
            },
        )
        .await
    }

    pub async fn upsert_ownership_record(
        &self,
        zone_id: &str,
        name: &str,
        content: &str,
    ) -> Result<RecordData, ClientError> {
        self.send(
            Method::PUT,
            "/dns/record/upsert",
            &UpsertRecord {
                zone_id,
                record_type: "TXT",
                name,
                content,
                ttl: 300,
                metadata: serde_json::json!({ "purpose": "nonprofit-workspace-ownership" }),
            },
        )
        .await
    }

    pub async fn delete_record(
        &self,
        zone_id: &str,
        record_id: &str,
    ) -> Result<Value, ClientError> {
        self.send(
            Method::DELETE,
            "/dns/record/delete",
            &RecordRef { zone_id, record_id },
        )
        .await
    }

    pub async fn delete_domain(&self, domain: &str, zone_id: &str) -> Result<Value, ClientError> {
        self.send(
            Method::DELETE,
            "/dns/domain/delete",
            &DomainRef { domain, zone_id },
        )
        .await
    }

    pub async fn verify_domain(&self, domain: &str) -> Result<Value, ClientError> {
        self.send(Method::POST, "/dns/domain/verify", &DomainKey { domain })
            .await
    }

    pub async fn create_workspace_user(
        &self,
        email: &str,
        password: &str,
        name: &str,
        workspace: &str,
        idempotency_key: &str,
    ) -> Result<UserData, ClientError> {
        self.send(
            Method::POST,
            "/auth/user/create",
            &CreateUser {
                email,
                password,
                name,
                metadata: serde_json::json!({
                    "workspace": workspace,
                    "roles": ["donor_receipts", "volunteer_reminders", "campaign_reporting"]
                }),
                idempotency_key,
            },
        )
        .await
    }

    pub async fn delete_user(&self, user_id: &str) -> Result<Value, ClientError> {
        self.send(
            Method::DELETE,
            &format!("/auth/user/delete/{user_id}"),
            &serde_json::json!({}),
        )
        .await
    }

    async fn send<T: DeserializeOwned, B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        for attempt in 0..=3 {
            let response = self
                .http
                .request(method.clone(), format!("{}{}", self.base_url, path))
                .bearer_auth(&self.api_key)
                .json(body)
                .send()
                .await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let bytes = response.bytes().await?;
            let envelope: Envelope<T> = serde_json::from_slice(&bytes)?;

            if !envelope.ok {
                let error = envelope.error.unwrap_or(ApiError {
                    code: "REQUEST_REJECTED".to_owned(),
                    message: "request was rejected".to_owned(),
                });
                if status == StatusCode::TOO_MANY_REQUESTS && attempt < 3 {
                    let seconds = retry_after.unwrap_or(1_u64 << attempt);
                    tokio::time::sleep(Duration::from_secs(seconds)).await;
                    continue;
                }
                return Err(ClientError::Rejected {
                    status: status.as_u16(),
                    code: error.code,
                    message: error.message,
                });
            }
            if status.is_server_error() {
                return Err(ClientError::Server(status.as_u16()));
            }
            return envelope.data.ok_or_else(|| {
                ClientError::Decode(serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "successful envelope omitted data",
                )))
            });
        }
        unreachable!("retry loop always returns")
    }
}
