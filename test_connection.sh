#!/bin/bash

# Number of concurrent connections to maintain
NUM_CONNECTIONS=100

# Duration of the test in seconds
DURATION=60

# URL of the load balancer
LOAD_BALANCER_URL="http://localhost:3000"

echo "Running wrk with $NUM_CONNECTIONS connections for $DURATION seconds to $LOAD_BALANCER_URL"

# Run wrk
wrk -t$NUM_CONNECTIONS -c$NUM_CONNECTIONS -d${DURATION}s $LOAD_BALANCER_URL