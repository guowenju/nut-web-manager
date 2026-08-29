#!/bin/sh
set -eu

data_dir=${NWM_DATA_DIR:-/data}
mkdir -p "$data_dir"
chmod 0700 "$data_dir"

exec "$@"
