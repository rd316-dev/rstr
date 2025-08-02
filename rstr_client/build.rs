#[cfg(windows)]
fn build_win_rc() {
    extern crate windres;
    use windres::Build;

    println!("cargo:rerun-if-changed=rstr.rc");
    if cfg!(windows) { 
        Build::new().compile("rstr.rc").unwrap();
    }
}

#[cfg(unix)]
fn build_win_rc() {}

fn main() {
    build_win_rc();
}