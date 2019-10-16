use std::collections::BTreeMap;
use std::iter::FromIterator;

use kube::api::{DeleteParams, ListParams, PostParams};
use kube::{api::Api, client::APIClient, config};
use serde_json::Value;
use serde_yaml;

use crate::tests::helpers;
use crate::{load_config_map, Gordo, GordoControllerConfigMap};

#[test]
fn test_controller_config_map_from() {
    let data = BTreeMap::from_iter(vec![(
        "deploy-image".to_owned(),
        "test-image-location".to_owned(),
    )]);
    let config = GordoControllerConfigMap::from(data);
    assert_eq!(&config.deploy_image, "test-image-location")
}

// We can create a gordo using the `example-gordo.yaml` file in the repo.
#[test]
fn test_create_gordo() {
    let client = helpers::client();
    let gordos = helpers::gordo_custom_resource_api(client);

    // Delete any gordos
    helpers::remove_gordos(&gordos);

    // Ensure there are no Gordos
    assert_eq!(gordos.list(&ListParams::default()).unwrap().items.len(), 0);

    // Apply the `gordo-example.yaml` file
    let config = helpers::gordo_example_config();
    let new_gordo =
        match gordos.create(&PostParams::default(), serde_json::to_vec(&config).unwrap()) {
            Ok(new_gordo) => new_gordo,
            Err(err) => panic!("Failed to create gordo with error: {:?}", err),
        };

    // Ensure there are now one gordos
    assert_eq!(gordos.list(&ListParams::default()).unwrap().items.len(), 1);

    // Delete the gordo
    if let Err(err) = gordos.delete(&new_gordo.metadata.name, &DeleteParams::default()) {
        panic!("Failed to delete gordo with error: {:?}", err);
    }

    // Back to zero gordos
    assert_eq!(gordos.list(&ListParams::default()).unwrap().items.len(), 0);
}

// Given an applied Gordo config which hasn't been submited to gordo-deploy job
// it should be able to pick those and submit them to gordo-deploy
#[test]
fn test_launch_waiting_gordos() {
    let client = helpers::client();
    let gordos = helpers::gordo_custom_resource_api(client.clone());

    // Delete any gordos
    helpers::remove_gordos(&gordos);

    // Apply the `gordo-example.yaml` file
    let config = helpers::gordo_example_config();
    let new_gordo =
        match gordos.create(&PostParams::default(), serde_json::to_vec(&config).unwrap()) {
            Ok(new_gordo) => new_gordo,
            Err(err) => panic!("Failed to create gordo with error: {:?}", err),
        };

    // No jobs waiting after applying config.
    let jobs = Api::v1Job(client.clone()).within("default");
    assert_eq!(jobs.list(&ListParams::default()).unwrap().items.len(), 0);

    // Launch the waiting config.
    let resource = helpers::gordo_custom_resource_api(client.clone());
    let config_map = load_config_map(client.clone(), "default");
    crate::launch_waiting_gordo_workflows(&resource, &client, "default", &config_map);

    // Now we should have one job.
    assert_eq!(jobs.list(&ListParams::default()).unwrap().items.len(), 1);

    // Delete all jobs
    crate::remove_gordo_deploy_jobs(&new_gordo, &client, "default");

    // And finally, we should have zero jobs
    std::thread::sleep(std::time::Duration::from_secs(5)); // Time for step above to finish
    assert_eq!(jobs.list(&ListParams::default()).unwrap().items.len(), 0);
}

#[test]
fn test_minor_version() {
    assert_eq!(crate::minor_version("0.33.0"), Some(33));
    assert_eq!(crate::minor_version("0.31.12"), Some(31));
    assert_eq!(crate::minor_version("0.abc.def"), None);
}
