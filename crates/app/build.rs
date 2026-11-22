// Gắn icon vào file .exe khi build trên Windows.
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/icon.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=cannot embed Windows icon: {e}");
        }
    }
}
