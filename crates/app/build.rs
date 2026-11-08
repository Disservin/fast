//! Embed the application icon in Windows executables.

/// Give Windows downloads, shortcuts and taskbar entries the Fast icon.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/app/fast.rc");
    println!("cargo:rerun-if-changed=../../assets/app/fast.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile(
            "../../assets/app/fast.rc",
            embed_resource::ParamsIncludeDirs(["../../assets/app"]),
        )
        .manifest_required()
        .expect("compile the Fast Windows icon resource");
    }
}
