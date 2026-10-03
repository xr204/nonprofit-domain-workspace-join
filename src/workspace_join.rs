use crate::infrai_client::{ClientError, InfraiClient};
use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Deserialize)]
pub struct JoinRequest {
    pub company_domain: String,
    pub workspace: String,
    pub employee_email: String,
    pub employee_name: String,
    pub initial_password: String,
    pub ownership_record_name: String,
    pub ownership_record_content: String,
    pub request_id: String,
}

#[derive(Debug, Serialize)]
pub struct JoinResult {
    pub decision: &'static str,
    pub workspace: String,
    pub employee_email: String,
    pub user_id: String,
    pub enabled_work: [&'static str; 3],
}

#[derive(Debug, Error)]
pub enum JoinError {
    #[error("employee email must belong to the company domain")]
    DomainMismatch,
    #[error(transparent)]
    Infrai(#[from] ClientError),
}

pub fn email_belongs_to_domain(email: &str, domain: &str) -> bool {
    let Some((local, email_domain)) = email.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && email_domain.eq_ignore_ascii_case(domain.trim_end_matches('.'))
        && !email_domain.is_empty()
}

pub async fn join_employee(
    client: &InfraiClient,
    input: JoinRequest,
) -> Result<JoinResult, JoinError> {
    if !email_belongs_to_domain(&input.employee_email, &input.company_domain) {
        return Err(JoinError::DomainMismatch);
    }

    let domain = client
        .add_domain(&input.company_domain, &input.workspace)
        .await?;
    let mut record_id = None;
    let mut user_id = None;
    let operation = async {
        let record = client
            .upsert_ownership_record(
                &domain.zone_id,
                &input.ownership_record_name,
                &input.ownership_record_content,
            )
            .await?;
        record_id = Some(record.record_id);
        client.verify_domain(&input.company_domain).await?;
        let user = client
            .create_workspace_user(
                &input.employee_email,
                &input.initial_password,
                &input.employee_name,
                &input.workspace,
                &input.request_id,
            )
            .await?;
        user_id = Some(user.id.clone());
        Ok::<_, ClientError>(user)
    }
    .await;

    let mut cleanup_error = None;
    if let Some(user_id) = &user_id {
        if let Err(error) = client.delete_user(user_id).await {
            cleanup_error = Some(error);
        }
    }
    if let Some(record_id) = &record_id {
        if let Err(error) = client.delete_record(&domain.zone_id, record_id).await {
            cleanup_error.get_or_insert(error);
        }
    }
    if let Err(error) = client
        .delete_domain(&input.company_domain, &domain.zone_id)
        .await
    {
        cleanup_error.get_or_insert(error);
    }
    if let Some(error) = cleanup_error {
        return Err(error.into());
    }
    let user = operation?;

    Ok(JoinResult {
        decision: "joined_verified_domain",
        workspace: input.workspace,
        employee_email: user.email,
        user_id: user.id,
        enabled_work: [
            "donor_receipts",
            "volunteer_reminders",
            "campaign_reporting",
        ],
    })
}

impl IntoResponse for JoinError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = match &self {
            Self::DomainMismatch => (StatusCode::UNPROCESSABLE_ENTITY, "domain_mismatch"),
            Self::Infrai(ClientError::Rejected { status, .. }) => (
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_REQUEST),
                "upstream_rejection",
            ),
            Self::Infrai(ClientError::Decode(_)) => (StatusCode::BAD_GATEWAY, "invalid_response"),
            Self::Infrai(ClientError::Transport(_) | ClientError::Server(_)) => {
                (StatusCode::BAD_GATEWAY, "upstream_unavailable")
            }
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": self.to_string() })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::email_belongs_to_domain;

    #[test]
    fn only_exact_nonprofit_domain_is_eligible() {
        assert!(email_belongs_to_domain("maya@riveraid.org", "riveraid.org"));
        assert!(email_belongs_to_domain("MAYA@RIVERAID.ORG", "riveraid.org"));
        assert!(!email_belongs_to_domain(
            "maya@volunteers.riveraid.org",
            "riveraid.org"
        ));
        assert!(!email_belongs_to_domain("maya@other.org", "riveraid.org"));
    }
}
