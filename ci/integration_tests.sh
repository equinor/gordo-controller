#!/bin/bash

export DEPLOY_IMAGE=docker.io/gordo/gordo-deploy
export DOCKER_REGISTRY=docker.io
export DEFAULT_DEPLOY_ENVIRONMENT=

SLEEP_TIMEOUT=10

cargo run &

sleep "$SLEEP_TIMEOUT"

cargo test --examples

for pid in `jobs -p`
do
    echo "Kill $pid"
    kill -s SIGTERM "$pid"
done
