#!/bin/bash
# SRT Receiver script
# Usage: ./run_receiver.sh [url]
# Default URL: srt://127.0.0.1:9000

cd "$(dirname "$0")"

URL="${1:-srt://127.0.0.1:9000}"

export LD_LIBRARY_PATH=./target/release:../bass-aes67/target/release:$LD_LIBRARY_PATH

./target/release/examples/test_srt_input "$URL"
