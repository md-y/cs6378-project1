#!/bin/bash

if [ -z "$1" ]; then
    echo "Please provide the node ID as the first argument."
    exit 1
fi

RUST_BACKTRACE=full cargo run "./cfg/utd.toml" "./cfg/$1.toml"
