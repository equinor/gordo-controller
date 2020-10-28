#!/bin/bash

set -e

MAX_ATTEMPTS=20
SLEEP_TIMEOUT=3
NAMESPACE=default

function usage {
    echo "Usage: $0 -m <max attempts> -s <sleep timeout> -n <namespace>"
    exit 0
}

function is_not_running {
    kubectl get pods -n "$NAMESPACE" -l app=gordo-controller -o custom-columns=STATUS:.status.phase | grep -i Running
}

while getopts "m:s:n:h" opt
do
	case $opt in
        m) MAX_ATTEMPTS="$OPTARG";;
        s) SLEEP_TIMEOUT="$OPTARG";;
        n) NAMESPACE="$OPTARG";;
        h) usage;;
	esac
done

attempt=0
until is_not_running
do
    sleep 3
    attempt=$((attempt+1))
    if [ "$attempt" -gt "$MAX_ATTEMPTS" ]; then
        echo "You have reached the maximum attempt #$MAX_ATTEMPTS"
        exit 1
    fi
done

exit 0
