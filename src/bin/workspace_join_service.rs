use axum::{extract::State, routing::post, Json, Router};
use nonprofit_domain_join::{
    infrai_client::InfraiClient,
    workspace_join::{join_employee, JoinError, JoinRequest, JoinResult},
};

#[tokio::main]
async fn main() {
    let client = InfraiClient::from_env().expect("INFRAI_API_KEY must be set");
    let app = Router::new()
        .route("/employees/join", post(join))
        .with_state(client);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("service port must be available");
    axum::serve(listener, app)
        .await
        .expect("service should run");
}

async fn join(
    State(client): State<InfraiClient>,
    Json(input): Json<JoinRequest>,
) -> Result<Json<JoinResult>, JoinError> {
    join_employee(&client, input).await.map(Json)
}
