#!/usr/bin/env bash
# 解析本次运行对应的 GitHub Release（幂等）。
#
#   分支推送（main） -> 预发布  v<清单版本>-dev.<run_number>
#   精确版本 tag     -> 正式发布 v<清单版本>（tag 必须与清单版本一致）
#
# 可选第 4 个参数：`with-commits` 会在创建 Release 之前收集「What's Changed」
# 提交列表（需要完整 git 历史）并作为初始正文；否则用占位正文。
#
# Release 以**公开状态**（draft=false）创建并连带创建真实 git tag。
# 刻意不用草稿：草稿的 tag 关联是最终一致的，并行构建的 tauri-action
# 会竞争它而报 "Release not found or created"。tauri-action 拿到数字
# release id 后直传资产，不需要再做 tag 查找。
#
#   ./scripts/resolve-release.sh <branch|tag> <ref-name> <run-number> [with-commits]
set -euo pipefail

REF_TYPE="${1:?ref 类型}"      # branch | tag
REF_NAME="${2:?ref 名}"        # main | v0.1.0
RUN_NUMBER="${3:?run number}"
WITH_COMMITS="${4:-}"          # with-commits | 空
REPO="${GITHUB_REPOSITORY:?}"
GH_TOKEN="${GH_TOKEN:?}"

# 清单同步：三个清单文件版本不一致时直接失败，不带着漂移的版本号发布
node scripts/check-versions.mjs
MANIFEST_VERSION="$(node -p "require('./package.json').version")"

if [[ "$REF_TYPE" == "tag" ]]; then
  TAG="$REF_NAME"
  VERSION="${TAG#v}"
  [[ "$VERSION" == "$MANIFEST_VERSION" ]] \
    || { echo "error: tag $TAG 与清单版本 $MANIFEST_VERSION 不一致" >&2; exit 1; }
  case "$VERSION" in *-*) PRERELEASE="true";; *) PRERELEASE="false";; esac
else
  # dev 构建把完整预发布版本（基数 + 运行号）写进打包产物，
  # 应用内显示的版本因此可以直接区分每一次构建
  VERSION="${MANIFEST_VERSION}-dev.${RUN_NUMBER}"
  TAG="v${VERSION}"
  PRERELEASE="true"
fi

# 幂等：tag 已有 Release 时直接复用（同一运行的重跑）
RELEASE_ID="$(gh api "repos/$REPO/releases?per_page=100" --jq ".[] | select(.tag_name == \"$TAG\") | .id" 2>/dev/null | head -n 1 || true)"
if [[ -n "$RELEASE_ID" ]]; then
  echo "复用既有 Release $RELEASE_ID（$TAG）"
else
  # 创建公开 Release；GitHub 会从 target_commitish 连带创建 git tag。
  # with-commits：Release 一创建就带「What's Changed」（下载表仍由
  # update-release-notes.sh 在打包矩阵上传完资产后补上）。
  BODY_ARGS=(-f "body=Preparing artifacts…")
  if [[ "$WITH_COMMITS" == "with-commits" ]]; then
    COMMITS_FILE="$(mktemp)"
    BODY_FILE="$(mktemp --suffix=.md)"
    trap 'rm -f "$COMMITS_FILE" "$BODY_FILE"' EXIT
    ./scripts/collect-commits.sh "$TAG" "$COMMITS_FILE"
    node scripts/release-notes-render.mjs "$TAG" - "$COMMITS_FILE" > "$BODY_FILE"
    BODY_ARGS=(-F "body=@$BODY_FILE")
  fi

  # 实测本仓库偶发 403「Resource not accessible by integration」：
  # 同一份代码、同样的令牌权限，相邻两次运行一成一败（#47/#49/#54/#61）。
  # 已排除权限声明与仓库设置问题，重试是对症的缓解；只对它重试，
  # 其它错误（tag 冲突等）重试也不会变好，直接失败。
  attempt=1
  delay=15
  while true; do
    set +e
    out="$(gh api "repos/$REPO/releases" \
      -f tag_name="$TAG" \
      -f target_commitish="$GITHUB_SHA" \
      -f name="$TAG" \
      "${BODY_ARGS[@]}" \
      -F draft=false \
      -F prerelease="$PRERELEASE" \
      --jq .id 2>&1)"
    rc=$?
    set -e
    if [[ $rc -eq 0 ]]; then
      RELEASE_ID="$out"
      break
    fi
    echo "创建 Release 失败（第 $attempt 次）："
    echo "$out"
    if ! grep -q "Resource not accessible by integration" <<<"$out"; then
      echo "该错误不属于可重试类型，直接失败" >&2
      exit "$rc"
    fi
    if [[ $attempt -ge 3 ]]; then
      echo "已重试 $attempt 次仍失败，放弃" >&2
      exit "$rc"
    fi
    echo "${delay} 秒后重试…"
    sleep "$delay"
    attempt=$((attempt + 1))
    delay=$((delay * 2))
  done
  echo "已创建 Release $RELEASE_ID（$TAG，prerelease=$PRERELEASE）"
fi

if [[ -z "$RELEASE_ID" ]]; then
  echo "error: 无法取得 Release $TAG 的数字 id" >&2
  exit 1
fi

echo "release_id=$RELEASE_ID" >> "$GITHUB_OUTPUT"
echo "tag=$TAG" >> "$GITHUB_OUTPUT"
echo "version=$VERSION" >> "$GITHUB_OUTPUT"
echo "prerelease=$PRERELEASE" >> "$GITHUB_OUTPUT"
