use kube::api::{DeleteParams, ListParams, PostParams};
use tokio_test::block_on;

mod helpers;

use gordo_controller::crd::gordo::Gordo;
use gordo_controller::crd::model::{filter_models_on_gordo, Model};
use gordo_controller::{GordoEnvironmentConfig, Config};
use gordo_controller::deploy_job::{deploy_job_name, create_deploy_job};

// We can create a gordo using the `example-gordo.yaml` file in the repo.
#[test]
fn test_create_gordo() {
    block_on(async {
        let client = helpers::client().await;
        let gordos = helpers::load_gordo_resource(client.clone(), "default");

        // Delete any gordos
        helpers::remove_gordos(&gordos).await;

        // Ensure there are no Gordos
        assert_eq!(gordos.list(&ListParams::default()).await.unwrap().items.len(), 0);

        // Apply the `gordo-example.yaml` file
        let gordo: Gordo = helpers::deserialize_config("example-gordo.yaml");
        let new_gordo = match gordos
            .create(&PostParams::default(), &gordo)
            .await
        {
            Ok(new_gordo) => new_gordo,
            Err(err) => panic!("Failed to create gordo with error: {:?}", err),
        };

        // Ensure there are now one gordos
        assert_eq!(gordos.list(&ListParams::default()).await.unwrap().items.len(), 1);

        let name = new_gordo.metadata.name.expect("metadata.name is empty");
        // Delete the gordo
        if let Err(err) = gordos.delete(&name, &DeleteParams::default()).await {
            panic!("Failed to delete gordo with error: {:?}", err);
        }

        // Back to zero gordos
        assert_eq!(gordos.list(&ListParams::default()).await.unwrap().items.len(), 0);
    })
}

#[test]
fn test_deploy_job_name() {
    let prefix = "gordo-dpl-";

    // Basic
    let suffix = "some-suffix";
    assert_eq!(&deploy_job_name(prefix, suffix), "gordo-dpl-some-suffix");

    // Really long suffix
    let mut suffix = std::iter::repeat("a").take(100).collect::<String>();
    suffix.push_str("required-suffix");
    let result = deploy_job_name(prefix, &suffix);
    assert_eq!(result.len(), 63);
    assert_eq!(
        &result,
        "gordo-dpl-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaarequired-suffix"
    );
}

#[test]
fn test_deploy_job_injects_project_version() {
    /*
    Ensure the resulting deploy jobs set the WORKFLOW_GENERATOR_PROJECT_VERSION environment variable
    inside the manifest of the deploy job.
    */
    let gordo: Gordo = helpers::deserialize_config("example-gordo.yaml");

    let config = Config::from_env_config(GordoEnvironmentConfig::default()).unwrap();

    let deploy_job = create_deploy_job(&gordo, &config).expect("Unable to create deploy job");

    let template = deploy_job.spec.unwrap().template;

    assert!(template.spec.unwrap().containers[0]
        .env
        .as_ref()
        .unwrap()
        .iter()
        .any(|ev| ev.name == "WORKFLOW_GENERATOR_PROJECT_REVISION"));
}

#[test]
fn test_filter_models_on_gordo() {
    // Setup Gordo with a project_revision in the status
    let mut gordo: Gordo = helpers::deserialize_config("example-gordo.yaml");

    let project_revision = "1234".to_owned();
    let new_status = gordo.status
        .map(|mut status| {
            status.project_revision = project_revision.clone();
            status
        });
    gordo.status = new_status ;

    // Make some Models
    let model: Model = helpers::deserialize_config("example-model.yaml");
    let mut models: Vec<Model> = std::iter::repeat(model).take(10).collect();

    // No models belong to this Gordo, they match OwnerReference but not the project_revision
    assert_eq!(filter_models_on_gordo(&gordo, &models).count(), 0);

    let metadata = &mut models[0].metadata;

    // Change one of the models to have a revision matching the Gordo
    let new_labels = metadata.labels.as_ref().map(|mut labels| {
            labels.insert("applications.gordo.equinor.com/project-version".to_owned(),project_revision);
            labels
        });
    metadata.labels = match new_labels {
        Some(l) => Some(*l),
        None => None,
    };
    assert_eq!(filter_models_on_gordo(&gordo, &models).count(), 1);
}
