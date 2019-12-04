use actix_web::{middleware, web, App, HttpServer};
use gordo_controller::{controller_init, views, GordoEnvironmentConfig};
use kube::config;
use log::info;

#[tokio::main]
async fn main() -> () {
    std::env::set_var("RUST_LOG", "info,kube=info");
    env_logger::init();

    let env_config = envy::from_env::<GordoEnvironmentConfig>().unwrap_or_default();
    info!("Starting with environment config: {:?}", &env_config);

    let kube_config = config::load_kube_config()
        .await
        .unwrap_or_else(|_| config::incluster_config().expect("Failed to get local kube config and incluster config"));

    let controller = controller_init(kube_config, env_config).await.unwrap();

    let bind_address = format!("{}:{}", &env_config.server_host, env_config.server_port);

    // Launch in new thread b/c HttpServer starts own async executor
    let handle = std::thread::spawn(move || {
        HttpServer::new(move || {
            App::new()
                .data(controller.clone())
                .wrap(middleware::Logger::default().exclude("/health"))
                .service(web::resource("/health").to(views::health))
                .service(web::resource("/gordos").to(views::gordos))
                .service(web::resource("/gordos/{name}").to(views::get_gordo))
                .service(web::resource("/models").to(views::models))
                .service(web::resource("/models/{gordo_name}").to(views::models_by_gordo))
        })
        .bind(&bind_address)
        .expect(&format!("Could not bind to '{}'", &bind_address))
        .run()
        .unwrap();
    });

    handle.join().unwrap()
}
