#!/bin/bash

SLEEP_TIMEOUT=10

cargo run &

sleep "$SLEEP_TIMEOUT"

cargo test --examples

for pid in `jobs -p`
do
    echo "Kill $pid"
    kill -s SIGTERM "$pid"
done
