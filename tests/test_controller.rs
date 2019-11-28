use kube::api::{DeleteParams, ListParams, PostParams};
use tokio_test::block_on;

mod helpers;

use gordo_controller::crd::gordo::load_gordo_resource;
use gordo_controller::{deploy_job::DeployJob};

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
