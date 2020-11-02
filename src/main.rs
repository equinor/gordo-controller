use actix_web::{middleware, web, App, HttpServer};
use gordo_controller::{controller_init, views, GordoEnvironmentConfig};
use kube::config;
use log::info;

#[actix_rt::main]
async fn main() -> () {
    //TODO do not forget about RUST_LOG env in all deployment scripts
    env_logger::init();

    let env_config: GordoEnvironmentConfig = match envy::from_env::<GordoEnvironmentConfig>() {
       Ok(config) => config,
       Err(error) => panic!("Failed to load environment config: {:#?}", error)
    };
    info!("Starting with environment config: {:?}", &env_config);

    let kube_config = config::load_kube_config()
        .await
        .unwrap_or_else(|_| config::incluster_config().expect("Failed to get local kube config and incluster config"));

    let bind_address = format!("{}:{}", &env_config.server_host, env_config.server_port);

    let controller = controller_init(kube_config, env_config).await.unwrap();

    HttpServer::new(move || {
        App::new()
            .data(controller.clone())
            .wrap(middleware::Logger::default().exclude("/health"))
            .wrap(middleware::Compress::default())
            .service(web::resource("/health").to(views::health))
            .service(web::resource("/gordos").to(views::gordos))
            .service(web::resource("/gordos/{name}").to(views::get_gordo))
            .service(web::resource("/models").to(views::models))
            .service(web::resource("/models/{gordo_name}").to(views::models_by_gordo))
    })
    .bind(&bind_address)
    .expect(&format!("Could not bind to '{}'", &bind_address))
    .run()
    .await
    .unwrap()
}
