use crate::crd::model::{filter_models_on_gordo, Model};
use crate::Gordo;
use actix_web::{http::StatusCode, error, web, HttpResponseBuilder, http, HttpRequest, HttpResponse};
use kube::{Client, Api};
use kube::api::ListParams;
use serde::Serialize;
use crate::errors::Error;

pub struct AppState {
    client: Client,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String
}

impl error::ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        HttpResponseBuilder::new(self.status_code())
            .json(ErrorResponse {
                error: self.to_string()
            })
    }

    fn status_code(&self) -> http::StatusCode {
        http::StatusCode::INTERNAL_SERVER_ERROR
    }
}

// Simple health check endpoint
pub async fn health(_req: HttpRequest) -> HttpResponse {
    HttpResponse::new(StatusCode::OK)
}

// List current gordos
pub async fn gordos(data: web::Data<AppState>, _req: HttpRequest) -> actix_web::Result<web::Json<Vec<Gordo>>, Error> {
    let gordo_api: Api<Gordo> = Api::default_namespaced(data.client.clone());
    let lp = ListParams::default();

    let gordo_list= gordo_api.list(&lp).await.map_err(Error::KubeError)?;
    let gordos: Vec<Gordo> = gordo_list.into_iter().collect();
    Ok(web::Json(gordos))
}

// Get a gordo by name
pub async fn get_gordo(data: web::Data<AppState>, name: web::Path<String>) -> actix_web::Result<web::Json<Gordo>, Error> {
    let gordo_api: Api<Gordo> = Api::default_namespaced(data.client.clone());
    let lp = ListParams::default();

    let name_str = name.as_str();
    let gordo_list= gordo_api.list(&lp).await.map_err(Error::KubeError)?;
    let gordo: Option<Gordo> = gordo_list
        .into_iter()
        .filter(|gordo| gordo.metadata.name == Some(name_str.to_string()))
        .nth(0);
    match gordo {
        Some(item)  => Ok(web::Json(item)),
        None => Err(Error::NotFound("gordo")),
    }
}

// List current models
pub async fn models(data: web::Data<AppState>, _req: HttpRequest) -> actix_web::Result<web::Json<Vec<Model>>, Error> {
    let model_api: Api<Model> = Api::default_namespaced(data.client.clone());
    let lp = ListParams::default();

    let model_list = model_api.list(&lp).await.map_err(Error::KubeError)?;
    let models: Vec<Model> = model_list.into_iter().collect();
    Ok(web::Json(models))
}

// List current models belonging to a specific Gordo at the same project revision number
pub async fn models_by_gordo(data: web::Data<AppState>, gordo_name: web::Path<String>) -> actix_web::Result<web::Json<Vec<Model>>, Error> {
    let gordo_api: Api<Gordo> = Api::default_namespaced(data.client.clone());
    let model_api: Api<Model> = Api::default_namespaced(data.client.clone());
    let lp = ListParams::default();

    let name_str = gordo_name.as_str();
    let gordo_list= gordo_api.list(&lp).await.map_err(Error::KubeError)?;
    // Get the gordo by name, can result in None
    let gordo_by_name: Option<Gordo> = gordo_list
        .into_iter()
        .filter(|gordo| gordo.metadata.name == Some(name_str.to_string()))
        .nth(0);

    // All models who's owner references have this gordo's name and matches the project revision number
    let models = match gordo_by_name {
        Some(gordo) => {
            let model_list = model_api.list(&lp).await.map_err(Error::KubeError)?;
            let all_models: Vec<Model> = model_list.into_iter().collect();
            filter_models_on_gordo(&gordo, &all_models)
                .map(ToOwned::to_owned)
                .collect()
        }
        None => Vec::with_capacity(0), // No models found for a Gordo which doesn't exist
    };

    Ok(web::Json(models))
}
