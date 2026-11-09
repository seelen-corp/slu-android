fn main() {
    // translations are embedded at compile time by `rust_i18n::i18n!`
    println!("cargo:rerun-if-changed=i18n");
    slint_build::compile("ui/app.slint").unwrap();
}
