GORDO_CONTROLLER_IMG_NAME := gordo-infrastructure/gordo-controller

gordo-controller:
	docker build . -f Dockerfile-controller -t $(GORDO_CONTROLLER_IMG_NAME)

.PHONY: gordo-controller
