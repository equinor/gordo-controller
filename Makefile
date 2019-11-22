export DOCKER_REGISTRY := docker.io
GORDO_CONTROLLER_IMG_NAME := equinor/gordo-controller

test:
	cargo test -- --test-threads=1

controller:
	docker build . -f Dockerfile-controller -t $(GORDO_CONTROLLER_IMG_NAME)

push-controller: controller
	export DOCKER_NAME=$(GORDO_CONTROLLER_IMG_NAME);\
	export DOCKER_IMAGE=$(GORDO_CONTROLLER_IMG_NAME);\
	./docker_push.sh

push-dev-controller: push-controller

push-prod-controller: export GORDO_PROD_MODE:="true"
push-prod-controller: push-controller

.PHONY: controller push-dev-controller push-prod-controller push-controller test
