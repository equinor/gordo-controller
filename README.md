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
