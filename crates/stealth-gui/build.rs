fn main() {
    // the few standard widgets used (text input, scroll views) take the dark Fluent style; the rest is custom
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("compiling the Slint UI");
}
