use super::*;

#[derive(Deserialize)]
pub struct AuthRequest {
    code: String,
    state: String,
}

pub async fn auth(
    data: web::Data<AppState>,
    params: web::Query<AuthRequest>,
    session: Session,
) -> actix_web::Result<HttpResponse> {
    /*println!(
        "Auth: session: {:?}",
        session.get::<String>(DISCORD_CSRF_COOKIE_NAME)
    );*/
    let res = match session.get::<CsrfToken>(DISCORD_CSRF_COOKIE_NAME) {
        Ok(Some(csrf)) if csrf.secret() == &params.state => data
            .oauth
            .as_ref()
            .expect("OAuth config")
            .exchange_code(AuthorizationCode::new(params.code.clone()))
            .request_async(&oauth_reqwest::async_http_client)
            .await
            .map_err(internal_error),
        Err(err) => Err(err.into()),
        _ => Err(bad_request("Anti-CSRF check failed")().into()),
    };
    session.remove(DISCORD_CSRF_COOKIE_NAME);
    let token = res?;

    let path = get_user_self();

    let auth = format!("Bearer {}", token.access_token().secret());
    let identity = oauth_reqwest::get(&data.reqwest, auth.clone(), path)
        .await
        .map_err(internal_error)?;
    let user: DiscordUser = serde_json::from_str(&identity).map_err(internal_error)?;
    let user_id: u64 = user.id.parse().map_err(internal_error)?;

    session.insert(DISCORD_USER_ID_COOKIE_NAME, user_id)?;
    let location: String = if let Some(location) = session.get(LOCATION_AFTER_AUTH)? {
        session.remove(LOCATION_AFTER_AUTH);
        location
    } else {
        "/".into()
    };

    Ok(HttpResponse::Found()
        //.content_type("text/plain; charset=utf-8")
        .append_header((header::LOCATION, location))
        .append_header((header::ACCESS_CONTROL_MAX_AGE, "0"))
        .finish())
}

fn get_user_self() -> String {
    format!("{}/users/@me", DISCORD_API_URL)
}
/*
fn get_user_guilds() -> String {
    format!("{}/users/@me/guilds", DISCORD_API_URL)
}

fn get_guild_member(guild: &str, user: &str) -> String {
    format!("{}/guilds/{}/members/{}", DISCORD_API_URL, guild, user)
}
*/
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct DiscordUser {
    id: String,
    username: String,
    discriminator: String,
    avatar: Option<String>,
    bot: Option<bool>,
    system: Option<bool>,
    mfa_enabled: Option<bool>,
    locale: Option<String>,
    verified: Option<bool>,
    email: Option<String>,
    flags: Option<u32>,
    premium_type: Option<u32>,
}

mod oauth_reqwest {
    use std::convert::TryInto;

    use futures::TryFutureExt;
    use oauth2::{HttpRequest, HttpResponse};

    pub async fn async_http_client(request: HttpRequest) -> Result<HttpResponse, reqwest::Error> {
        assert_eq!(request.method().as_str(), "POST");

        let client = reqwest::Client::builder()
            // Following redirects opens the client up to SSRF vulnerabilities.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        let response = client.execute(request.try_into()?).await?;

        let response: http::Response<reqwest::Body> = response.into();
        let (parts, body) = response.into_parts();
        let buf = http_body_util::BodyExt::collect(body)
            .await
            .map(|buf| buf.aggregate())?;
        let body = bytes::buf::IntoIter::new(buf).collect();

        Ok(HttpResponse::from_parts(parts, body))
    }
    pub async fn get(
        client: &reqwest::Client,
        auth: String,
        path: String,
    ) -> Result<String, reqwest::Error> {
        let request = client
            .request(reqwest::Method::GET, &path)
            .header("Authorization", auth)
            .build()?;

        client
            .execute(request)
            .and_then(|response| response.text())
            .await
    }
}
