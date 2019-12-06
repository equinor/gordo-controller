use kube::api::{DeleteParams, ListParams, PostParams};
use tokio_test::block_on;

mod helpers;

use gordo_controller::crd::gordo::{load_gordo_resource, Gordo};
use gordo_controller::deploy_job::DeployJob;
use gordo_controller::GordoEnvironmentConfig;
use std::collections::HashMap;

// We can create a gordo using the `example-gordo.yaml` file in the repo.
#[test]
fn test_create_gordo() {
    block_on(async {
        let client = helpers::client().await;
        let gordos = load_gordo_resource(&client, "default");

        // Delete any gordos
        helpers::remove_gordos(&gordos).await;

        // Ensure there are no Gordos
        assert_eq!(gordos.list(&ListParams::default()).await.unwrap().items.len(), 0);

        // Apply the `gordo-example.yaml` file
        let config = helpers::example_config("example-gordo.yaml");
        let new_gordo = match gordos
            .create(&PostParams::default(), serde_json::to_vec(&config).unwrap())
            .await
        {
            Ok(new_gordo) => new_gordo,
            Err(err) => panic!("Failed to create gordo with error: {:?}", err),
        };

        // Ensure there are now one gordos
        assert_eq!(gordos.list(&ListParams::default()).await.unwrap().items.len(), 1);

        // Delete the gordo
        if let Err(err) = gordos.delete(&new_gordo.metadata.name, &DeleteParams::default()).await {
            panic!("Failed to delete gordo with error: {:?}", err);
        }

        // Back to zero gordos
        assert_eq!(gordos.list(&ListParams::default()).await.unwrap().items.len(), 0);
    })
}

#[test]
fn test_minor_version() {
    assert_eq!(gordo_controller::minor_version("0.33.0"), Some(33));
    assert_eq!(gordo_controller::minor_version("0.31.12"), Some(31));
    assert_eq!(gordo_controller::minor_version("0.abc.def"), None);
}

#[test]
fn test_deploy_job_name() {
    let prefix = "gordo-dpl-";

    // Basic
    let suffix = "some-suffix";
    assert_eq!(&DeployJob::deploy_job_name(prefix, suffix), "gordo-dpl-some-suffix");

    // Really long suffix
    let mut suffix = std::iter::repeat("a").take(100).collect::<String>();
    suffix.push_str("required-suffix");
    let result = DeployJob::deploy_job_name(prefix, &suffix);
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
    let gordo: Gordo = serde_json::from_value(helpers::example_config("example-gordo.yaml")).unwrap();

    let deploy_job = DeployJob::new(&gordo, &GordoEnvironmentConfig::default());

    let env: Vec<HashMap<String, String>> =
        serde_json::from_value(deploy_job.manifest["spec"]["template"]["spec"]["containers"][0]["env"].clone())
            .unwrap();

    assert!(env
        .iter()
        .any(|kv| kv.get("name") == Some(&"WORKFLOW_GENERATOR_PROJECT_VERSION".to_owned())));
}
