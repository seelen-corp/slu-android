#[path = "system-modules/mod.rs"]
mod system_modules;

use system_modules::apps;

slint::slint! {
    import { ListView } from "std-widgets.slint";

    export component MainWindow inherits Window {
        in property <[string]> app_titles: [];

        ListView {
            for title in app_titles: Text {
                text: title;
                font-size: 18px;
                color: black;
            }
        }
    }
}

fn apps_model() -> slint::ModelRc<slint::SharedString> {
    let titles: Vec<slint::SharedString> = apps::get_installed_apps()
        .into_iter()
        .map(|app| app.title.into())
        .collect();
    slint::ModelRc::new(slint::VecModel::from(titles))
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    log::info!("Hello World from slu_launcher (pure Rust Android launcher skeleton)");

    apps::init(&app);
    slint::android::init(app).unwrap();

    let main_window = MainWindow::new().unwrap();
    main_window.set_app_titles(apps_model());

    let weak_window = main_window.as_weak();
    apps::subscribe(move || {
        let weak_window = weak_window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(window) = weak_window.upgrade() {
                window.set_app_titles(apps_model());
            }
        })
        .ok();
    });

    main_window.run().unwrap();
}
