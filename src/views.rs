use crate::Controller;
use actix_web::{dev::HttpResponseBuilder, http::StatusCode, web, HttpRequest, HttpResponse};

// Simple health check endpoint
pub async fn health(_req: HttpRequest) -> HttpResponse {
    HttpResponse::new(StatusCode::OK)
}

// List current gordos
pub async fn gordos(data: web::Data<Controller>, _req: HttpRequest) -> HttpResponse {
    let gordos = data.gordo_state().await;
    HttpResponseBuilder::new(StatusCode::OK).json(gordos)
}

// List current models
pub async fn models(data: web::Data<Controller>, _req: HttpRequest) -> HttpResponse {
    let models = data.model_state().await;
    HttpResponseBuilder::new(StatusCode::OK).json(models)
}
