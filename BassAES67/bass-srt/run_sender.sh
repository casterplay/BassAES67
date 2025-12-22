#!/bin/bash
# SRT Sender script
# Usage: ./run_sender.sh [codec]
# Codecs: pcm, opus, mp2, flac (default: opus)

cd "$(dirname "$0")"

CODEC="${1:-opus}"

export LD_LIBRARY_PATH=./target/release:../bass-aes67/target/release:$LD_LIBRARY_PATH

./target/release/examples/srt_sender_framed --codec "$CODEC"
