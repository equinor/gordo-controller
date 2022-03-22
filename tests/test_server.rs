use gordo_controller::views::AppState;

use actix_web::web::Json;
use actix_web::{http::StatusCode, test, web};
use gordo_controller::views;
use gordo_controller::{crd::gordo::Gordo, crd::model::Model};

mod helpers;

#[tokio::test]
async fn test_view_health() {
    let req = test::TestRequest::default().to_http_request();
    let resp = views::health(req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_view_gordos() {
    let data = app_state().await;

    let req = test::TestRequest::default().to_http_request();
    let resp: Json<Vec<Gordo>> = views::gordos(data, req).await.expect("Unable to get gordos");
    assert_eq!(resp.0.len(), 0);
}

#[tokio::test]
async fn test_view_models() {
    let data = app_state().await;

    let req = test::TestRequest::default().to_http_request();
    let resp: Json<Vec<Model>> = views::models(data, req).await.expect("Unable to get models");
    assert_eq!(resp.0.len(), 0);
}

// Helper for just this module: loading app state for testing
async fn app_state() -> web::Data<AppState> {
    let client = helpers::client().await;
    web::Data::new(views::AppState {
        client: client.clone(),
    })
}
