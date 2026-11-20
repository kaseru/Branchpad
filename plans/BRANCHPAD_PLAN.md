# Plan: Branchpad – app macOS: git như Fork + soạn file như VSCode

## Mục tiêu
App macOS gốc, siêu nhẹ, viết bằng Rust. Git làm trên máy (không tài khoản) như Fork, kèm trình soạn file gọn như VSCode.
Đích đo: bộ cài < 30MB; RAM lúc rảnh < 150MB; mở repo ~10k file / ~20k commit < 1 giây.

## Công nghệ
- Rust + GPUI (bộ giao diện của Zed) + gpui-kit (Longbridge), ghim phiên bản
- Trình soạn: bộ soạn code của gpui-kit, tô màu bằng tree-sitter (khoảng 10 ngôn ngữ: js/ts/tsx, json, rust, swift, kotlin, python, md, yaml, html, css)
- Git: gọi lệnh `git` có sẵn trên máy, dùng SSH key/keychain sẵn có, không hỏi mật khẩu
- Theo dõi file bằng sự kiện của macOS, không polling
- Đồ thị nhánh: tham khảo cách xếp đường nhánh của karyalabs/bonsai (MIT)
- Build và chạy thử trên node DG_Mac

## Bố cục
- Trên cùng: mỗi repo một tab
- Trái: Files (cây file) · Changes · nhánh local · remote · tag · stash · worktree
- Giữa: đồ thị + danh sách commit; dưới là file của commit + diff
- Phải: trình soạn nhiều tab (kéo rộng/hẹp, đóng được)
- Bấm 1 lần vào file = xem diff ở giữa; bấm đúp = mở ở cột phải (file đang làm thì sửa được, file của commit cũ thì chỉ đọc)

## GĐ0 – Chuẩn bị 👤
- [ ] 👤 Tạo repo kaseru/branchpad trên GitHub, để DG_Mac thấy repo
- [ ] 👤 Cài Rust vào ổ MDATA (đặt thư mục rustup/cargo trên /Volumes/MDATA vì ổ hệ thống còn ~8GB); Xcode đã có, kiểm thêm bộ công cụ Metal

## GĐ1 – Khung app
- [ ] Cửa sổ chính đúng bố cục trên, tab nhiều repo, nhớ các repo đã mở
- [ ] Mở repo từ dòng lệnh: `branchpad .`
- [ ] Sáng/tối theo hệ thống, phím tắt chuẩn macOS

## GĐ2 – Trình soạn file
- [ ] Cây file: tải dần, ẩn file trong .gitignore, tô màu file đã đổi, tạo/đổi tên/xoá file và thư mục, mở trong Finder
- [ ] Mở nhiều tab, sửa và lưu (Cmd+S), chấm báo chưa lưu, hỏi khi đóng tab chưa lưu
- [ ] Hoàn tác/làm lại, tìm/thay trong file, nhảy tới dòng, bật/tắt chú thích dòng, thụt lề tự động, gập code
- [ ] Cmd+P mở file nhanh, Cmd+Shift+F tìm chữ trong cả thư mục
- [ ] File bị sửa từ app khác thì tự tải lại (nếu chưa sửa trong app)
- [ ] Lề trái đánh dấu dòng thêm/sửa/xoá so với git

## GĐ3 – File thay đổi + commit
- [ ] Changes: unstaged/staged/untracked, tự cập nhật
- [ ] Diff; stage/unstage/bỏ thay đổi theo file và theo đoạn
- [ ] Commit (Cmd+Enter), sửa commit cuối
- [ ] Fetch/pull/push, báo số commit đi trước/đi sau, lỗi hiện rõ ràng

## GĐ4 – Nhánh, remote, tag, stash, worktree
- [ ] Checkout/tạo/xoá/đổi tên nhánh, theo dõi nhánh remote
- [ ] Remote: thêm/xoá, fetch từng remote
- [ ] Tag: tạo/xoá/push
- [ ] Stash: tạo/áp dụng/pop/xoá
- [ ] Worktree: xem, thêm, xoá; mở worktree thành tab mới

## GĐ5 – Lịch sử + đồ thị
- [ ] Danh sách commit tải dần, chỉ vẽ dòng đang hiện; tìm theo message/tác giả/hash
- [ ] Đồ thị nhánh bên trái danh sách commit
- [ ] Chọn commit: thông tin + danh sách file + diff; bấm đúp file thì mở bản của commit đó (chỉ đọc)
- [ ] Chuột phải commit: checkout, tạo nhánh/tag, cherry-pick, revert, reset (soft/mixed/hard), tạo patch, copy hash
- [ ] Lịch sử từng file + blame

## GĐ6 – Merge, rebase, patch, xung đột
- [ ] Merge nhánh vào nhánh hiện tại
- [ ] Rebase lên nhánh khác; tiếp tục/bỏ qua/huỷ
- [ ] Rebase tương tác: đổi thứ tự, gộp, sửa message, bỏ commit
- [ ] Apply patch (file .patch/.diff, cả dạng mail patch)
- [ ] Xung đột: liệt kê file lỗi, mở trong trình soạn, nút chọn bản mình / bản kia / cả hai cho từng khối, đánh dấu đã xử lý

## Trạng thái (2026-11-21)
Mã nguồn GĐ1–GĐ6 đã viết (xem README.md), **chưa biên dịch**: máy viết code không có Rust và ổ đĩa đầy. Việc còn lại:
- [ ] 👤 Người làm: trên DG_Mac chạy `cargo test -p branchpad-git -p branchpad-core` và `cargo build -p branchpad`, gửi log lỗi biên dịch để sửa (chủ yếu ở chỗ gọi API GPUI/gpui-component đã ghim `=0.2.2` / `=0.4.0`)
- [ ] 👤 Người làm: chạy thử từng mục GĐ1–GĐ6 trên repo thật, đo và ghi số vào README

Khác plan ở bản đầu: dấu dòng thêm/sửa/xoá hiện ở thanh dọc cạnh trình soạn (cả file) thay vì lề trái từng dòng; chú thích dòng (Cmd+/) áp cho dòng có con trỏ; gập code tuỳ vào bộ soạn của gpui-component.

## Không làm (bản đầu)
Nối tài khoản GitHub/GitLab, gợi ý code và báo lỗi khi gõ, terminal, extension, debug, submodule/LFS, AI.

## Tiêu chí nghiệm thu
- Đạt các đích đo ở mục Mục tiêu (ghi số đo thật trong README)
- Pull/push qua SSH không hỏi mật khẩu
- Sửa file bằng app khác → Changes và tab đang mở tự cập nhật, không polling
- Merge/rebase có xung đột xử lý trọn trong app, không cần mở terminal
- Build bản release sạch cảnh báo trên DG_Mac
