use crate::crd::model::{filter_models_on_gordo, Model};
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

// List current models belonging to a specific Gordo at the same project revision number
pub async fn models_by_gordo(data: web::Data<Controller>, gordo_name: web::Path<String>) -> web::Json<Vec<Model>> {
    // Get the gordo by name, can result in None
    let gordo_by_name: Option<Gordo> = data
        .gordo_state()
        .await
        .into_iter()
        .filter(|gordo| &gordo.metadata.name == gordo_name.as_str())
        .nth(0);

    // All models who's owner references have this gordo's name and matches the project revision number
    let models = match gordo_by_name {
        Some(gordo) => {
            let all_models = data.model_state().await;
            filter_models_on_gordo(&gordo, &all_models)
                .map(ToOwned::to_owned)
                .collect()
        }
        None => Vec::with_capacity(0), // No models found for a Gordo which doesn't exist
    };

    web::Json(models)
}
