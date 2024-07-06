#!/bin/bash

SERVICE1_DIR="service1"
SERVICE2_DIR="service2"
WEBSOCKET_DIR="websocket_server"

SERVICE1_DOCKERFILE="$SERVICE1_DIR/Dockerfile"
SERVICE2_DOCKERFILE="$SERVICE2_DIR/Dockerfile"
WEBSOCKET_DOCKERFILE="$WEBSOCKET_DIR/Dockerfile"

DOCKER_COMPOSE_FILE="docker-compose.yml"

# Create directories for services
mkdir -p $SERVICE1_DIR
mkdir -p $SERVICE2_DIR
mkdir -p $WEBSOCKET_DIR

# for service1
cat <<EOF >$SERVICE1_DOCKERFILE
FROM python:3.8-slim

WORKDIR /app

RUN echo "1" > index.html

CMD ["python3", "-m", "http.server", "8000"]
EOF

# for service2
cat <<EOF >$SERVICE2_DOCKERFILE
FROM python:3.8-slim

WORKDIR /app

RUN echo "2" > index.html

CMD ["python3", "-m", "http.server", "8080"]
EOF



# Create docker-compose.yml
cat <<EOF >$DOCKER_COMPOSE_FILE
version: '3.8'

services:
  service1:
    build:
      context: ./service1
    ports:
      - "8000:8000"

  service2:
    build:
      context: ./service2
    ports:
      - "8080:8080"

EOF


docker-compose up --build
