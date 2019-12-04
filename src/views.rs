use crate::crd::model::Model;
use crate::{Controller, Gordo};
use actix_web::{http::StatusCode, web, HttpRequest, HttpResponse};

// Simple health check endpoint
pub async fn health(_req: HttpRequest) -> HttpResponse {
    HttpResponse::new(StatusCode::OK)
}

// List current gordos
pub async fn gordos(data: web::Data<Controller>, _req: HttpRequest) -> web::Json<Vec<Gordo>> {
    web::Json(data.gordo_state().await)
}

// Get a gordo by name
pub async fn get_gordo(data: web::Data<Controller>, name: web::Path<String>) -> web::Json<Option<Gordo>> {
    let gordo = data
        .gordo_state()
        .await
        .into_iter()
        .filter(|gordo| gordo.metadata.name == name.as_str())
        .nth(0);
    web::Json(gordo)
}

// List current models
pub async fn models(data: web::Data<Controller>, _req: HttpRequest) -> web::Json<Vec<Model>> {
    web::Json(data.model_state().await)
}

// List current models belonging to a specif Gordo
pub async fn models_by_gordo(data: web::Data<Controller>, gordo_name: web::Path<String>) -> web::Json<Vec<Model>> {
    let models = data
        .model_state()
        .await
        .into_iter()
        .filter(|model| {
            model
                .metadata
                .ownerReferences
                .iter()
                .any(|owner_ref| owner_ref.name == gordo_name.as_str())
        })
        .collect();
    web::Json(models)
}
