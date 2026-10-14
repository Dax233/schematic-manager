#!/usr/bin/env bash
# 收集一个 Release tag 对应的「更新内容」提交列表。
#
#   ./scripts/collect-commits.sh <TAG> <输出文件>
#
# 由 scripts/resolve-release.sh（创建 Release 前的初始正文）与
# scripts/update-release-notes.sh（打包完成后的正式说明）共用。
#
# 收集范围：
#   预发布（v<ver>-dev.<n>） -> 上一个**任意种类**的 tag（dev 或正式版，
#                               按语义版本取更近者；正式版排在自己那批
#                               dev 之后）。刚发完正式版的第一个 dev 因此
#                               会收集自该正式版以来的全部提交，之后的
#                               dev 保持增量。
#   正式版（v<ver>）         -> 上一个**正式版** tag（严格 vX.Y.Z；最新的
#                               dev tag 通常与正式版在同一个提交上，拿它
#                               当起点会让「更新内容」一片空白）
set -euo pipefail

TAG="${1:?tag}"
OUT="${2:?输出文件}"

# 确保完整历史（不只是浅检出的 HEAD）与全部 tag 在场，
# 否则 git log <prev>..HEAD 无法回溯到上一个 tag。
if git rev-parse --is-shallow-repository | grep -q true; then
  git fetch --unshallow --tags --force --quiet
else
  git fetch --tags --force --quiet
fi

# 当前 TAG 总是被排除，这样重跑时也能选到上一个。
if [[ "$TAG" == *-dev.* ]]; then
  # dev 构建：以上一个任意种类的 tag 为起点，按语义版本取更近者。
  # 不能用 git 的 version:refname 排序：它把 dev tag 排在自己的正式版
  # 之上（v0.2.0-dev.49 > v0.2.0），会把新周期的第一个 dev 锚到上一周期
  # 的最后一个 dev，让已发布过的提交漏进说明。这里给每个候选构造排序键，
  # 让正式版排在自己的 dev 之后（后缀 "zzzz" > "dev"），再用 GNU sort -rV
  # 按语义版本排序。
  PREV_TAG="$(
    git tag --list 'v*' \
      | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+(-dev\.[0-9]+)?$' \
      | grep -v -x "$TAG" \
      | sed -E 's/^v([0-9]+\.[0-9]+\.[0-9]+)$/\1-zzzz &/; t; s/^v([0-9]+\.[0-9]+\.[0-9]+-dev\.[0-9]+)$/\1 &/' \
      | sort -rV \
      | head -n 1 \
      | cut -d' ' -f2- || true
  )"
else
  # 正式版：只取严格 vX.Y.Z。dev tag 永远排在自己的正式版之下，
  # 且正式版 tag 通常与最后一个 dev tag 在同一个提交上，
  # 以同提交的 dev 为起点会得到空范围、空白的「更新内容」。
  PREV_TAG="$(
    git tag --list 'v*' --sort=-version:refname \
      | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' \
      | grep -v -x "$TAG" \
      | head -n 1 || true
  )"
fi

if [[ -n "$PREV_TAG" ]]; then
  echo "收集自 $PREV_TAG 以来的提交"
  # 只取主题行，最新的在前，跳过合并提交
  git log --no-merges --format='%s' "$PREV_TAG..HEAD" > "$OUT"
else
  echo "没有找到上一个 tag，列出全部提交"
  git log --no-merges --format='%s' > "$OUT"
fi
