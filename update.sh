#!/bin/sh
set -e
if [ -d .git ]; then
  git pull --ff-only
fi
./install.sh
echo "update done"
