# gordo-controller
Gordo controller

[![CI](https://github.com/equinor/gordo-controller/workflows/CI/badge.svg)](https://github.com/equinor/gordo-controller/actions)

## Developer manual

### Run tests locally

Install and start [minikube](https://minikube.sigs.k8s.io/docs/start).

Run k8s API proxy:
```
kubectl proxy --port=8080
```

Export necessary env variables:
```bash
export KUBERNETES_SERVICE_HOST=localhost
export KUBERNETES_SERVICE_PORT=8080
```

Build the docker image:
```
eval $(minikube docker-env)
docker build -f Dockerfile-controller -t equinor/gordo-controller:latest .
kubectl apply -k k8s/base
```

Run tests:
```
cargo test --tests -- --test-threads=1
```

Environment variables:

| Name                         | Type    | Description                                                                    |
| ---------------------------- | ------- | ------------------------------------------------------------------------------ |
| DEPLOY\_IMAGE                | String  | Deploy Job docker image.                                                       |
|                              |         | Example: `auroradevacr.azurecr.io/gordo-infrastructure/gordo-deploy`           |
| DEPLOY\_REPOSITORY           | String  | Deploy Job docker image with registry.                                         |
|                              |         | Example: `gordo-infrastructure/gordo-deploy`                                   |
| SERVER\_PORT                 | Integer | HTTP server listening port. Example: `8080`                                    |
| SERVER\_HOST                 | String  | HTTP server listening host. Example: `localhost`                               |
| DOCKER\_REGISTRY             | String  | Docker registry. Example: `auroradevacr.azurecr.io`                            |
| DEFAULT\_DEPLOY\_ENVIRONMENT | HashMap | Default gordo's environment variables.                                         |
|                              |         | Example: `{"ARGO_SERVICE_ACCOUNT": "workflow-runner"}`                         |
| RESOURCES\_LABELS            | HashMap | Deploy Job labels. Example: `{"app": "gordo_deployment"}`                      |
| DEPLOY\_JOB\_RO\_FS          | Boolean | Set up `.security_context.read_only_root_filesystem` for deploy Job if `true`  |
