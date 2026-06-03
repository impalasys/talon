use talon_server::Options;

#[test]
fn options_default_has_startup_timeout() {
    assert!(Options::default().startup_timeout.as_secs() > 0);
}

