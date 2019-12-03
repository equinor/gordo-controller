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

// List current models
pub async fn models(data: web::Data<Controller>, _req: HttpRequest) -> web::Json<Vec<Model>> {
    web::Json(data.model_state().await)
}
