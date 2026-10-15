#!/usr/bin/env bash
# 发布准备好的 Release：渲染双语下载表与「What's Changed」提交列表，
# 然后刷新 Release 说明（prerelease 标志保持 prepare 解析出的值）。
#
#   ./scripts/update-release-notes.sh <release-id> <tag> <prerelease>
#
# 提交收集范围见 scripts/collect-commits.sh 的头部注释。
set -euo pipefail

RELEASE_ID="${1:?release id}"
TAG="${2:?tag}"
PRERELEASE="${3:?prerelease}"
REPO="${GITHUB_REPOSITORY:?}"
GH_TOKEN="${GH_TOKEN:?}"

# 取全部已上传资产；有不认识的名字就在渲染阶段直接失败。
# 按数字 release id 取（REST）：gh release view 会把参数解析成 tag，
# 而草稿/刚创建的 release 用 tag 查可能 404。
ASSETS_JSON="$(gh api "repos/$REPO/releases/$RELEASE_ID" --jq '[.assets[].name]' | jq -c .)"

COMMITS_FILE="$(mktemp)"
NOTES_FILE="$(mktemp --suffix=.md)"
trap 'rm -f "$COMMITS_FILE" "$NOTES_FILE"' EXIT

# 用 bash 显式调用：脚本在 git 里没有可执行位（Windows 上 core.filemode=false，
# 新建的 .sh 一律记为 100644），直接 ./ 调用在 Linux runner 上会 Permission denied。
bash ./scripts/collect-commits.sh "$TAG" "$COMMITS_FILE"
node scripts/release-notes-render.mjs "$TAG" "$ASSETS_JSON" "$COMMITS_FILE" > "$NOTES_FILE"

# Release 由 resolve-release.sh 以公开状态创建，这里只刷新正文。
# 用 REST PATCH 按数字 id 更新：gh release edit 会把数字 id 当成 tag 解析，
# 即使 release 存在也会报 "release not found"。
gh api -X PATCH "repos/$REPO/releases/$RELEASE_ID" \
  -F body=@"$NOTES_FILE" \
  -F prerelease="$PRERELEASE" \
  --silent
