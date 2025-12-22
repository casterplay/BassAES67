#!/bin/bash
# Run the C# SRT test application
# Usage: ./run.sh [srt://host:port]

cd "$(dirname "$0")"

URL="${1:-srt://127.0.0.1:9000}"

export LD_LIBRARY_PATH=../bass-srt/target/release:../bass-aes67/target/release:../bass24-linux/x64:$LD_LIBRARY_PATH

dotnet run -- "$URL"
