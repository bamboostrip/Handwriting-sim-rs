#!/usr/bin/env bash
set -e
VERSION="${GITHUB_REF_NAME#v}"
TAG="${GITHUB_REF_NAME}"
awk -v ver="$VERSION" '
    $0 ~ "^## \\[" ver "\\]" { found = 1; next }
    found && /^## / { exit }
    found { print }
' CHANGELOG.md > /tmp/changelog-section.md
if [ ! -s /tmp/changelog-section.md ]; then
    echo "::error::CHANGELOG.md 中未找到版本 [${VERSION}] 小节，请先补充更新日志"
    exit 1
fi
{
    echo "## 更新内容"
    cat /tmp/changelog-section.md
    echo ""
    # 模板中 {TAG} 占位符替换为实际 tag，产物下载链接即指向本版资产
    sed "s/{TAG}/${TAG}/g" packaging/release-body.md
} > RELEASE_NOTES.md
echo "== RELEASE_NOTES.md 生成完成 =="
