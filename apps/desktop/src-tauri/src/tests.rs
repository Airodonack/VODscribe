//! The specs of the app's root: how it starts and what its window may load.

mod start;

#[test]
fn webview_args_keep_wrys_and_add_the_drivers() {
    assert_eq!(
        super::webview_args(
            "--disable-features=IgnoreDuplicateNavs,Prewarm --remote-debugging-port=0 --enable-logging"
        ),
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IgnoreDuplicateNavs,Prewarm \
         --remote-debugging-port=0 --enable-logging"
    );
    assert_eq!(super::webview_args(""), "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
}

/// The policy built in lets pictures come from no server on this
/// computer; the image server the tests name, and only it, is let in
/// (lib.rs `context`: "the page's policy also letting pictures come
/// from the image server the end-to-end tests name, on this computer").
#[test]
fn pictures_come_from_this_computer_only_from_the_tests_image_server() {
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let built_in: tauri::utils::config::Csp = serde_json::from_value(config["app"]["security"]["csp"].clone()).unwrap();
    let img_src = |csp: &tauri::utils::config::Csp| match csp {
        tauri::utils::config::Csp::DirectiveMap(d) => Vec::<String>::from(d["img-src"].clone()).join(" "),
        tauri::utils::config::Csp::Policy(p) => p.clone(),
    };
    let release = img_src(&built_in);
    assert!(release.contains("https://img.vodscribe.com"), "{release}");
    for local in ["127.0.0.1", "localhost", "[::1]"] {
        assert!(!release.contains(local), "{release}");
    }
    let mut csp = Some(built_in.clone());
    super::allow_images_from(&mut csp, "http://127.0.0.1:41723/");
    assert_eq!(img_src(csp.as_ref().unwrap()), format!("{release} http://127.0.0.1:41723"));
    let mut csp = Some(built_in);
    super::allow_images_from(&mut csp, "not a server");
    assert_eq!(img_src(csp.as_ref().unwrap()), release);
}
