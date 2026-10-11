//! Ресурсы Windows: иконка и метаданные приложения (встраиваются в exe).

fn main() {
    println!("cargo:rerun-if-changed=resources/klavium.ico");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("resources/klavium.ico");
    resource.set(
        "FileDescription",
        "Klavium — мгновенное исправление раскладки",
    );
    resource.set("ProductName", "Klavium");
    resource.set("CompanyName", "Klavium");
    resource.set("LegalCopyright", "© Klavium contributors");
    resource.set("OriginalFilename", "klavium.exe");
    resource.set("FileVersion", &version);
    resource.set("ProductVersion", &version);
    // Числовая версия «major.minor.patch» → 0xMMMM_mmmm_pppp_0000.
    let mut parts = version
        .split('.')
        .filter_map(|part| part.parse::<u64>().ok());
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    let patch = parts.next().unwrap_or(0);
    let fixed = (major << 48) | (minor << 32) | (patch << 16);
    resource.set_version_info(winresource::VersionInfo::FILEVERSION, fixed);
    resource.set_version_info(winresource::VersionInfo::PRODUCTVERSION, fixed);
    if let Err(error) = resource.compile() {
        // Ресурсы — украшение: если компилятора ресурсов нет, просто предупреждаем.
        println!("cargo:warning=не удалось встроить ресурсы приложения: {error}");
    }
}
