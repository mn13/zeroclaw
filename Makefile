COMPOSE := docker compose -f docker/docker-compose.yml
AGENT_IMAGE := zeroclaw-agent

.PHONY: build up down restart gateway web

## build       — Build all images (agent, gateway, web)
build:
	docker build -f docker/Dockerfile.zc -t $(AGENT_IMAGE) .
	$(COMPOSE) build

## up          — Build everything and start gateway + web
up: build
	$(COMPOSE) up -d

## down        — Stop gateway, web, and all managed agent containers
down:
	$(COMPOSE) down
	@containers=$$(docker ps -aq --filter "label=zeroclaw.managed=true"); \
	if [ -n "$$containers" ]; then \
		echo "Stopping zeroclaw agent containers..."; \
		docker stop $$containers; \
		docker rm $$containers; \
	fi

## restart     — Full restart: stop everything, rebuild, start
restart: down up

## gateway     — Build and start only the gateway (+ agent image)
gateway:
	docker build -f docker/Dockerfile.zc -t $(AGENT_IMAGE) .
	$(COMPOSE) build gateway
	$(COMPOSE) up -d gateway

## web         — Build and start only the web UI
web:
	$(COMPOSE) build web
	$(COMPOSE) up -d web
