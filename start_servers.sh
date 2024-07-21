#!/bin/bash

# Usage function
usage() {
    echo "Usage: $0 <number_of_services>"
    exit 1
}

# Check if the correct number of arguments is provided
if [ "$#" -ne 1 ]; then
    usage
fi

NUM_SERVICES=$1

DOCKER_COMPOSE_FILE="docker-compose.yml"
TOML_FILE="development.toml"

# Clean up previous directories and files
rm -rf service_*
rm -f $DOCKER_COMPOSE_FILE
rm -f $TOML_FILE

# Create directories and files for services
for i in $(seq 1 $NUM_SERVICES); do
    SERVICE_DIR="service_$i"
    mkdir -p $SERVICE_DIR
    cat <<EOF >$SERVICE_DIR/index.html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Service $i</title>
    <style>
body {
    font-family: "Trebuchet MS", "Lucida Sans Unicode", "Lucida Grande","Lucida Sans", Arial, sans-serif;
    text-align: center;
    height: 100vh;
    overflow: hidden;
    background: #e4edfb;
}
.container {
    display: flex;
    justify-content: center;
    height: 100%;
    align-items: center;
}
.box {
    width: 450px;
    aspect-ratio: 1 / 1;
    border-radius: 50px;
    display: flex;
    justify-content: center;
    flex-direction: column;
    align-items: center;
    background: #e4edfb;
    box-shadow: 20px 20px 60px #c2c9d5, -20px -20px 60px #ffffff;
}
h3 {
    font-weight: 500;
    font-size: 1.5rem;
    color: #333;
}
.service-number {
    font-weight: 700;
    font-size: 4.5rem;
}
    </style>
</head>
<body>
    <div class='container'>
        <div class='box'>
            <h3>Service</h3>
            <span class="service-number">$i</span>
        </div>
    </div>
</body>
</html>
EOF

    # Create a Dockerfile for each service using Nginx
    cat <<EOF >$SERVICE_DIR/Dockerfile
FROM nginx:alpine

COPY index.html /usr/share/nginx/html/index.html
COPY nginx.conf /etc/nginx/nginx.conf
EOF

    # Create an Nginx configuration file for HTTP/1.1
    cat <<EOF >$SERVICE_DIR/nginx.conf
events {}


http {
    log_format main '$remote_addr - $remote_user [$time_local] "$request" '
                      '$status $body_bytes_sent "$http_referer" '
                      '"$http_user_agent" "$http_x_forwarded_for" '
                      'rt=$request_time';

    access_log /var/log/nginx/access.log main;

    server {
        listen 8000 http2;
        server_name localhost;

        location / {
            root /usr/share/nginx/html;
            index index.html;

            # Enable keep-alive connections
            keepalive_timeout 0;
        }

        location /nginx_status {
            stub_status on;
            access_log off;
            allow 127.0.0.1;
            deny all;
        }
    }
}
EOF

done

# Create docker-compose.yml
cat <<EOF >$DOCKER_COMPOSE_FILE
version: '3.8'

services:
EOF

# Add services to docker-compose.yml
for i in $(seq 1 $NUM_SERVICES); do
    cat <<EOF >>$DOCKER_COMPOSE_FILE
  service_$i:
    build:
      context: ./service_$i
    ports:
      - "$((8000 + i - 1)):8000"
EOF
done

# Create development.toml
cat <<EOF >$TOML_FILE
listen_addr = "127.0.0.1:3000"
algorithm = "round-robin"
# algorithm = "random"
# algorithm = "least-connection"
monitoring_interval = 10
health_check_interval = 5
EOF

# Add backends to development.toml
for i in $(seq 1 $NUM_SERVICES); do
    cat <<EOF >>$TOML_FILE
[[backends]]
name = "service-$i"
addr = "127.0.0.1:$((8000 + i - 1))"
EOF
done

# Build and start the containers
docker compose up --build

# Clean up created service directories
for i in $(seq 1 $NUM_SERVICES); do
    rm -rf "service_$i"
done