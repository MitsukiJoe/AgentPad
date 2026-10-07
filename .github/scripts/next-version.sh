#!/usr/bin/env bash
# 从标准输入读取已有 tag 列表，输出下一个版本号（不带 v）。
# 规则：patch+1，逢 10 进位（patch→minor→major，major 不限）；无 tag 或最新 < 0.1.0 时取基线。
set -euo pipefail

baseline="0.1.0"
tags="$(grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' || true)"
latest="$(printf '%s\n' "$tags" | sed -n 's/^v//p' | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)"

if [ -z "$latest" ]; then
  version="$baseline"
else
  IFS='.' read -r major minor patch <<< "$latest"
  major=$((10#$major)); minor=$((10#$minor)); patch=$((10#$patch))
  if (( major == 0 && minor < 1 )); then
    version="$baseline"
  else
    patch=$((patch + 1))
    if (( patch >= 10 )); then patch=0; minor=$((minor + 1)); fi
    if (( minor >= 10 )); then minor=0; major=$((major + 1)); fi
    version="$major.$minor.$patch"
  fi
fi

# 必须严格大于所有已有 tag，且 tag 不得已存在
top="$(printf '%s\nv%s\n' "$tags" "$version" | sed -n 's/^v//p' | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)"
if [ "$top" != "$version" ] || printf '%s\n' "$tags" | grep -qx "v$version"; then
  echo "computed v$version is not greater than all existing tags or already exists" >&2
  exit 1
fi
echo "$version"
