#!/usr/bin/env bash
# Bridges: Run a daily standup across all project agents
#
# Usage:
#   chmod +x daily-standup.sh
#   ./daily-standup.sh [project-id]

set -euo pipefail

PROJECT="${1:-}"

echo "=== Bridges Daily Standup ==="
echo ""

# Step 1: Sync latest state
echo "Syncing..."
bridges sync ${PROJECT:+--project "$PROJECT"}

# Step 2: Show everyone's progress
echo ""
echo "--- Current Progress ---"
echo "Read .shared/PROGRESS.md and .shared/CHANGELOG.md in the project checkout."

# Step 3: Broadcast standup question
echo ""
echo "--- Asking all agents for standup update ---"
bridges broadcast "Standup: What did you work on? What are you working on next? Any blockers?" ${PROJECT:+--project "$PROJECT"}

echo ""
echo "=== Standup Complete ==="
