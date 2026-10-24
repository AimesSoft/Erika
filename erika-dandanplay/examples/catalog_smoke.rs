//! Read-only network check, also runnable as an Android shell executable.
fn main() {
    match erika_dandanplay::details_from(17527, true) {
        Ok(details) => println!(
            "Public details: summary={}, episodes={}",
            !details.summary.is_empty(),
            details.episodes.len()
        ),
        Err(error) => println!("Public details failed: {}", error.message),
    }
    match erika_dandanplay::bangumi::details(393010) {
        Ok(details) => println!(
            "Bangumi details: summary={}, episodes={}",
            !details.summary.is_empty(),
            details.episodes.len()
        ),
        Err(error) => println!("Bangumi details failed: {}", error.message),
    }
}
