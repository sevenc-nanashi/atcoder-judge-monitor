use std::{collections::HashMap, sync::Arc};

use anyhow::ensure;

use crate::{debug, info, question, store};

pub async fn main() -> anyhow::Result<()> {
    let username = dialoguer::Input::<String>::new()
        .with_prompt(question!("Enter your username"))
        .interact()?;
    let password = dialoguer::Password::new()
        .with_prompt(question!("Enter your password"))
        .interact()?;
    let cookie_path = store::get_cookie_path();
    let _ = fs_err::remove_file(&cookie_path);

    info!("Logging in...");

    let cookie_store = reqwest_cookie_store::CookieStore::default();
    let cookie_store = reqwest_cookie_store::CookieStoreMutex::new(cookie_store);
    let cookie_store = Arc::new(cookie_store);
    let client = reqwest::Client::builder()
        .user_agent("AtCoderJudgeMonitor/0.1")
        .cookie_store(true)
        .cookie_provider(Arc::clone(&cookie_store))
        .build()?;
    let login_html: String = client
        .get("https://atcoder.jp/login")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let login_html = scraper::Html::parse_document(&login_html);
    let csrf_token = login_html
        .select(&scraper::Selector::parse("input[name=csrf_token]").unwrap())
        .next()
        .unwrap()
        .value()
        .attr("value")
        .unwrap();
    debug!("CSRF token: {}", csrf_token);

    let mut params = HashMap::new();
    params.insert("username", username.clone());
    params.insert("password", password.clone());
    params.insert("csrf_token", csrf_token.to_string());
    let login_result = client
        .post("https://atcoder.jp/login")
        .header("Referer", "https://atcoder.jp/login")
        .form(&params)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    ensure!(
        login_result.contains(format!(r#"var userScreenName = "{}";"#, username).as_str()),
        "Failed to login"
    );
    info!("Logged in as {}", &username);

    let cookies = cookie_store.lock().unwrap();

    cookie_store::serde::json::save(&cookies, &mut fs_err::File::create(&cookie_path)?)
        .map_err(|err| anyhow::anyhow!("Failed to save cookies: {}", err))?;

    debug!("Cookies saved to {:?}", cookie_path);

    Ok(())
}
