#!/usr/bin/env bash
# 把一个已经构建好的静态二进制打成 .deb。
#
#   build-deb.sh <二进制> <版本号> <amd64|arm64> [输出目录]
#
# 只接受 musl 静态二进制 —— control 里没有 Depends。
set -euo pipefail

BIN="${1:-}"
VERSION="${2:-}"
ARCH="${3:-}"
OUTPUT_DIR="${4:-dist}"

die() {
  echo "错误: $*" >&2
  exit 1
}

[[ -f "$BIN" ]] || die "给的二进制不存在: ${BIN:-<空>}"
VERSION="${VERSION#v}"
[[ -n "$VERSION" && "$VERSION" =~ ^[0-9][0-9A-Za-z.+:~-]*$ ]] || die "版本号无效: ${VERSION:-<空>}"
case "$ARCH" in
  amd64 | arm64) ;;
  *) die "架构只认 amd64 / arm64，给的是: ${ARCH:-<空>}" ;;
esac
command -v dpkg-deb >/dev/null 2>&1 || die "缺少 dpkg-deb"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR="$(cd "$OUTPUT_DIR" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT
STAGE="${WORK}/root"

install -D -m 0755 "$BIN" "${STAGE}/usr/bin/funpub"
install -D -m 0644 "${ROOT}/LICENSE" "${STAGE}/usr/share/doc/funpub/copyright"

INSTALLED_SIZE="$(du -sk "${STAGE}/usr" | awk '{print $1}')"
mkdir -p "${STAGE}/DEBIAN"
# 没有 Depends：二进制是 musl 静态链接的，装上就能跑。
# ca-certificates 是 Recommends 而不是 Depends：编进二进制的 webpki 根证书可验证公网站点，
# 装上 ca-certificates 后还能识别系统与企业私有 CA。
cat >"${STAGE}/DEBIAN/control" <<EOF
Package: funpub
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Maintainer: farfarfun <farfarfun@qq.com>
Installed-Size: ${INSTALLED_SIZE}
Recommends: ca-certificates
Homepage: https://github.com/farfarfun/funpub
Description: Multi-channel artifact publishing CLI
 funpub uploads, downloads, checks, and signs URLs for artifacts across
 repositories such as Aliyun Packages generic registries, reading encrypted
 credentials from funsecret.
 Statically linked; no runtime dependencies.
EOF

PACKAGE="${OUTPUT_DIR}/funpub_${VERSION}_${ARCH}.deb"
dpkg-deb --root-owner-group --build "${STAGE}" "${PACKAGE}" >/dev/null
echo "$PACKAGE"
