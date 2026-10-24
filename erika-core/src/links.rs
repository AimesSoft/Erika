//! Addresses the user opens outside the app, in the system's browser.

use bern::CommandError;

/// Open `url`, an http or https address, in the system's browser.
pub(crate) fn open(url: &str) -> Result<(), CommandError> {
    let url = url.trim();
    let web = url.starts_with("https://") || url.starts_with("http://");
    if !web || url.chars().any(char::is_whitespace) {
        return Err(CommandError::new(
            "invalid_url",
            format!("not a web address: {url}"),
        ));
    }
    launch(url)
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn launch(url: &str) -> Result<(), CommandError> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    let mut child = command
        .arg(url)
        .spawn()
        .map_err(|error| CommandError::new("platform_unavailable", error.to_string()))?;
    // Reap the launcher once it hands the address over.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn launch(_url: &str) -> Result<(), CommandError> {
    Err(CommandError::new(
        "platform_unavailable",
        "this platform cannot open web addresses yet",
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_web_addresses_open() {
        for url in ["ftp://example.com", "file:///etc/hosts", "https://a b", ""] {
            assert_eq!(super::open(url).unwrap_err().code, "invalid_url");
        }
    }

    #[test]
    fn the_version_is_the_apps() {
        let manifest = include_str!("../../Bern.toml");
        let app = &manifest[manifest.find("[app]").unwrap()..];
        let line = app.lines().find(|line| line.starts_with("version")).unwrap();
        assert_eq!(line, format!("version = \"{}\"", env!("CARGO_PKG_VERSION")));
    }
}
