use std::{collections::HashMap, sync::Arc};

use crate::{debug, info, question, store};

pub async fn main() -> anyhow::Result<()> {
    let username = dialoguer::Input::<String>::new()
        .with_prompt(question!("Enter your username"))
        .interact()?;
    let password = dialoguer::Password::new()
        .with_prompt(question!("Enter your password"))
        .interact()?;

    info!("Logging in...");

    let cookie_store = reqwest_cookie_store::CookieStore::default();
    let cookie_store = reqwest_cookie_store::CookieStoreMutex::new(cookie_store);
    let cookie_store = Arc::new(cookie_store);
    let client = reqwest::Client::builder()
        .cookie_provider(Arc::clone(&cookie_store))
        .build()?;
    let login_html: String = client
        .get("https://atcoder.jp/login")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let csrf_token =
        regex::Regex::new(r#"<input type="hidden" name="csrf_token" value="([^"]+)" />"#)
            .unwrap()
            .captures(&login_html)
            .unwrap()
            .get(1)
            .unwrap()
            .as_str();
    debug!("CSRF token: {}", csrf_token);

    let mut params = HashMap::new();
    params.insert("username", username.clone());
    params.insert("password", password.clone());
    params.insert("csrf_token", csrf_token.to_string());
    client
        .post("https://atcoder.jp/login")
        .header("Referer", "https://atcoder.jp/login")
        .form(&params)
        .send()
        .await?
        .error_for_status()?;

    info!("Logged in as {}", &username);

    let path = store::get_cookie_path();

    let cookies = cookie_store.lock().unwrap();

    cookies
        .save_json(&mut fs_err::File::create(&path)?)
        .map_err(|err| anyhow::anyhow!("Failed to save cookies: {}", err))?;

    debug!("Cookies saved to {:?}", path);

    Ok(())
}
