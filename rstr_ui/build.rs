fn main() {
    println!("cargo:rerun-if-changed=ui/");
    unsafe { std::env::set_var("SLINT_BACKEND", "winit-skia") };
    slint_build::compile("ui/app-window.slint").unwrap();
}