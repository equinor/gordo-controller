use tokio_test::block_on;

use actix_web::web::Json;
use actix_web::{http::StatusCode, test, web};
use gordo_controller::{controller_init, load_kube_config, views, Controller, GordoEnvironmentConfig};
use gordo_controller::{crd::gordo::Gordo, crd::model::Model};

#[test]
fn test_view_health() {
    block_on(async {
        let req = test::TestRequest::default().to_http_request();
        let resp = views::health(req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    })
}

#[test]
fn test_view_gordos() {
    block_on(async {
        let data = app_state().await;

        let req = test::TestRequest::default().to_http_request();
        let resp: Json<Vec<Gordo>> = views::gordos(data, req).await;
        assert_eq!(resp.0.len(), 0);
    })
}

#[test]
fn test_view_models() {
    block_on(async {
        let data = app_state().await;

        let req = test::TestRequest::default().to_http_request();
        let resp: Json<Vec<Model>> = views::models(data, req).await;
        assert_eq!(resp.0.len(), 0);
    })
}

// Helper for just this module: loading app state for testing
async fn app_state() -> web::Data<Controller> {
    let kube_config = load_kube_config().await;
    let controller = controller_init(kube_config, GordoEnvironmentConfig::default())
        .await
        .unwrap();
    web::Data::new(controller)
}
