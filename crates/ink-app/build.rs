//! Puts the logo and a name into the Windows executable, so Explorer, the task bar and the properties window show
//! them. Nothing to do for other systems. A failure (no resource compiler on the machine) only costs the icon, so it
//! is a warning and the build goes on.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/logo/inkwell.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("../../assets/logo/inkwell.ico");
    resource.set("ProductName", "inkwell");
    resource.set("FileDescription", "inkwell, a drawing and note app");
    resource.set("OriginalFilename", "inkwell.exe");
    if let Err(error) = resource.compile() {
        println!("cargo:warning=the icon was not added to the executable: {error}");
    }
}
