#!/bin/bash

# AIMP Version Manager
# Synchronizes version numbers across the repository.

if [ -z "$1" ]; then
    echo "Usage: ./scripts/version_manager.sh <NEW_VERSION>"
    echo "Example: ./scripts/version_manager.sh 0.1.1"
    exit 1
fi

NEW_VERSION=$1

echo "Updating AIMP to version $NEW_VERSION..."

sed -i '' "s/^version = \".*\"/version = \"$NEW_VERSION\"/" aimp_node/Cargo.toml
sed -i '' "s/# AIMP (AI Mesh Protocol) v.*/# AIMP (AI Mesh Protocol) v$NEW_VERSION/" README.md
sed -i '' "s/__version__ = \".*\"/__version__ = \"$NEW_VERSION\"/" aimp_testbed/aimp_client/__init__.py
sed -i '' "s/version=\".*\"/version=\"$NEW_VERSION\"/" aimp_testbed/setup.py

echo "AIMP version synchronized to $NEW_VERSION"
