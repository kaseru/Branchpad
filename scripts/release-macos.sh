#!/bin/bash
# Build bản release, đóng gói Branchpad.app, thay bản trong /Applications rồi mở lại app.
#
#   scripts/release-macos.sh             # build + cài + mở lại
#   scripts/release-macos.sh --no-install  # chỉ build + đóng gói (target/release/bundle/Branchpad.app)
#
# Đổi thư mục cài bằng BRANCHPAD_INSTALL_DIR (mặc định /Applications).
# Bước thay app chạy ở tiến trình tách riêng: chạy script từ khung AI của chính Branchpad vẫn được,
# vì khi Branchpad tắt thì bước cài không bị tắt theo. Nhật ký bước cài: /tmp/branchpad-install.log.
set -euo pipefail

BUNDLE_ID="vn.kaseru.branchpad"
LOG="/tmp/branchpad-install.log"

# ---- Bước cài (tiến trình tách riêng): tắt app cũ, chép app mới, mở lại ----
if [[ "${1:-}" == "--install" ]]; then
    src="$2"
    dest="$3"
    exe="$dest/Contents/MacOS/branchpad"
    echo "$(date '+%F %T') cài $src -> $dest"
    if pgrep -f "$exe" >/dev/null; then
        # Thoát êm để app kịp lưu repo / file / phiên AI đang mở.
        osascript -e "tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1 || true
        for _ in $(seq 1 100); do
            pgrep -f "$exe" >/dev/null || break
            sleep 0.1
        done
        if pgrep -f "$exe" >/dev/null; then
            echo "app không tự thoát sau 10 giây, buộc tắt"
            pkill -f "$exe" || true
            sleep 1
        fi
    fi
    # Giữ bản cũ đến khi chép xong bản mới; lỗi giữa chừng thì trả bản cũ lại.
    rm -rf "$dest.old"
    [[ -d "$dest" ]] && mv "$dest" "$dest.old"
    if ditto "$src" "$dest"; then
        rm -rf "$dest.old"
    else
        echo "chép thất bại, trả lại bản cũ"
        rm -rf "$dest"
        [[ -d "$dest.old" ]] && mv "$dest.old" "$dest"
    fi
    open "$dest"
    echo "$(date '+%F %T') xong"
    exit 0
fi

# ---- Build + đóng gói ----
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
build="$(git rev-list --count HEAD 2>/dev/null || echo 1)"
app="$root/target/release/bundle/Branchpad.app"
dest="${BRANCHPAD_INSTALL_DIR:-/Applications}/Branchpad.app"

echo "==> cargo build --release (v$version, build $build)"
cargo build --release -p branchpad

echo "==> đóng gói $app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/branchpad "$app/Contents/MacOS/branchpad"
cp assets/icon/icon.icns "$app/Contents/Resources/icon.icns"
sed -e "s/__VERSION__/$version/" -e "s/__BUILD__/$build/" scripts/macos/Info.plist >"$app/Contents/Info.plist"
codesign --force --sign - "$app"

if [[ "${1:-}" == "--no-install" ]]; then
    echo "==> xong: $app"
    exit 0
fi

echo "==> thay $dest và mở lại (nhật ký: $LOG)"
# fork + setsid: bước cài thoát khỏi nhóm tiến trình hiện tại, không chết theo Branchpad.
perl -MPOSIX -e 'fork and exit; POSIX::setsid(); exec @ARGV' \
    /bin/bash "$root/scripts/release-macos.sh" --install "$app" "$dest" >>"$LOG" 2>&1 </dev/null
