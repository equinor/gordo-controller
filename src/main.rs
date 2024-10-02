use actix_web::{middleware, web, App, HttpServer};
use actix_web_prom::PrometheusMetricsBuilder;
use errors::Error;
use gordo_controller::{crd, errors, init_gordo_controller, views, Config};
use kube::client::Client;
use log::{info, warn};
use prometheus::Registry;
use std::env::vars;

#[actix_rt::main]
async fn main() -> Result<(), errors::Error> {
    //TODO do not forget about RUST_LOG env in all deployment scripts
    env_logger::init();

    let gordo_config = Config::from_envs(vars()).unwrap();
    info!("Starting with config: {:?}", gordo_config);
    let bind_address = format!("{}:{}", &gordo_config.server_host, gordo_config.server_port);

    let client = Client::try_default().await.map_err(Error::KubeError)?;
    let controller = init_gordo_controller(client.clone(), gordo_config);

    let registry = Registry::new();
    crd::metrics::custom_metrics(&registry);
    let prometheus = PrometheusMetricsBuilder::new(crd::metrics::METRICS_NAMESPACE)
        .endpoint("/metrics")
        .registry(registry)
        .build()
        .unwrap();

    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(views::AppState { client: client.clone() }))
            .wrap(prometheus.clone())
            .wrap(middleware::Logger::default().exclude("/health").exclude("/metrics"))
            .wrap(middleware::Compress::default())
            .service(web::resource("/health").to(views::health))
            .service(web::resource("/gordos").to(views::gordos))
            .service(web::resource("/gordos/{name}").to(views::get_gordo))
            .service(web::resource("/models").to(views::models))
            .service(web::resource("/models/{gordo_name}").to(views::models_by_gordo))
    })
    .bind(&bind_address)
    .expect(&format!("Could not bind to '{}'", &bind_address));

    tokio::select! {
        _ = server.run() => {
            info!("actix exited");
        }
        _ = controller => {
            warn!("gordo controller drained");
        }
    }

    Ok(())
}
