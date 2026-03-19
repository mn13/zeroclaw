ROOT_DIR := $(shell cd $(dir $(lastword $(MAKEFILE_LIST))) && pwd)
COMPOSE := ZCGW_HOST_AGENTS_DIR=$(ROOT_DIR)/docker/agents docker compose -f docker/docker-compose.yml
AGENT_IMAGE := zeroclaw-agent

.PHONY: build up down restart gateway web local local-gateway local-web local-down

## ─── Docker targets ──────────────────────────────────────

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

## ─── Local (host) targets ────────────────────────────────

## local       — Build and run gateway + web on host
local: local-gateway local-web

## local-gateway — Build and run gateway on host (port 8080)
local-gateway:
	@mkdir -p local/agents
	@if [ ! -f local/zcgw.toml ]; then \
		cp local/zcgw.example.toml local/zcgw.toml; \
		echo "Created local/zcgw.toml from example"; \
	fi
	cargo build --release -p zcgw
	ZCGW_CONFIG_PATH=local/zcgw.toml \
	ZCGW_HOST_MODE=true \
	ZCGW_AGENTS_DIR=local/agents \
	ZCGW_HOST_AGENTS_DIR=$(ROOT_DIR)/local/agents \
	ZCGW_DOCKER_IMAGE=$(AGENT_IMAGE) \
	cargo run --release -p zcgw

## local-web   — Run web UI dev server on host (port 5173)
local-web:
	cd web && npm run dev

## local-down  — Stop managed agent containers (gateway stops with Ctrl-C)
local-down:
	@containers=$$(docker ps -aq --filter "label=zeroclaw.managed=true"); \
	if [ -n "$$containers" ]; then \
		echo "Stopping zeroclaw agent containers..."; \
		docker stop $$containers; \
		docker rm $$containers; \
	fi
