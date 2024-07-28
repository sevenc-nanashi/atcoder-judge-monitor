use std::path::PathBuf;
use std::sync::Arc;

use crate::warn;

pub fn get_config_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap();
    path.push("atcoder-judge-monitor");
    path
}

pub fn get_cookie_path() -> PathBuf {
    let mut path = get_config_dir();
    path.push("cookies.json");
    path
}

pub fn create_http_client() -> Option<reqwest::Client> {
    let Ok(cookie_file) = fs_err::File::open(get_cookie_path()) else {
        warn!("Failed to open cookie file");
        return None;
    };

    let cookie_store =
        reqwest_cookie_store::CookieStore::load_json(std::io::BufReader::new(cookie_file))
            .unwrap_or_default();
    let cookie_store = reqwest_cookie_store::CookieStoreMutex::new(cookie_store);
    let agent = reqwest::Client::builder()
        .cookie_provider(Arc::new(cookie_store))
        .build()
        .unwrap();
    Some(agent)
}
