slint::slint! {
    export component MainWindow inherits Window {
        background: #3B82F6;
        Text {
            text: "Hello World";
            color: white;
        }
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    log::info!("Hello World from slu_launcher (pure Rust Android launcher skeleton)");

    slint::android::init(app).unwrap();

    let main_window = MainWindow::new().unwrap();
    main_window.run().unwrap();
}
