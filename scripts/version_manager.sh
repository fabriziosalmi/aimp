#!/bin/bash

# AIMP Version Manager
# Synchronizes version numbers across the repository.

if [ -z "$1" ]; then
    echo "Usage: ./scripts/version_manager.sh <NEW_VERSION>"
    echo "Example: ./scripts/version_manager.sh 0.1.1"
    exit 1
fi

NEW_VERSION=$1
DATE=$(date +%Y-%m-%d)

echo "🚀 Updating AIMP to version $NEW_VERSION..."

# 1. Update Cargo.toml
sed -i '' "s/^version = \".*\"/version = \"$NEW_VERSION\"/" aimp_node/Cargo.toml

# 2. Update README.md header
sed -i '' "s/# AIMP (AI Mesh Protocol) v.*/# AIMP (AI Mesh Protocol) v$NEW_VERSION/" README.md

# 3. Update SPEC.md header
sed -i '' "s/# AIMP v.* Protocol Specification/# AIMP v$NEW_VERSION Protocol Specification/" SPEC.md

# 4. Update CHANGELOG.md (Add new entry if needed or update latest)
# For now, we update the [Unreleased] or the latest version block
sed -i '' "s/## \[.*\] - .*/## \[$NEW_VERSION\] - $DATE/" CHANGELOG.md

echo "✅ AIMP version synchronized to $NEW_VERSION"
