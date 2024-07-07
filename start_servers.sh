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

# Create Dockerfiles and directories for services
for i in $(seq 1 $NUM_SERVICES); do
    SERVICE_DIR="service_$i"
    mkdir -p $SERVICE_DIR
    cat <<EOF >$SERVICE_DIR/Dockerfile
FROM python:3.8-slim

WORKDIR /app

COPY index.html .

CMD ["python3", "-m", "http.server", "8000"]
EOF


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

EOF

# Add backends to development.toml
for i in $(seq 1 $NUM_SERVICES); do
    cat <<EOF >>$TOML_FILE
[[backends]]
name = "service-$i"
addr = "127.0.0.1:$((8000 + i - 1))"
EOF
done

docker-compose up --build
