use std::time::Duration;
#[allow(unused_imports)]
use std::{collections::BTreeMap, sync::Arc};

use actix_service::Service;
use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    error::InternalError,
    middleware, web, App, HttpRequest, HttpResponse, HttpServer,
};
#[cfg(feature = "fo_data")]
use fo_data::FoData;
use futures::{
    future::{FutureExt, TryFutureExt},
    Future,
};
use oauth2::{basic, EndpointNotSet, EndpointSet};
use tokio::sync::Mutex;

use self::restrict::restrict;
use crate::{bridge, config, critters_db::CrittersDb, database::SledDb};

mod avatar;
mod char_action;
mod dir;
mod gm;
mod meta;
mod restrict;
mod stats;

#[cfg(feature = "fo_data")]
mod data;
#[cfg(feature = "map_viewer")]
mod map_viewer;

const STATIC_PATH: &str = "./static/";

async fn index(
    _req: HttpRequest,
    session: actix_session::Session,
    data: web::Data<AppState>,
) -> actix_web::Result<HttpResponse> {
    let body = match meta::get_user_id(&session) {
        Some(user_id) => {
            let (name_string, max_rank) = match meta::get_user_record(&data, user_id).await {
                Ok(record) => (
                    match &record.nick {
                        Some(nick) => format!(r#"{} ({})"#, record.name, nick),
                        None => format!(r#"{}"#, record.name),
                    },
                    record.ranks.first().cloned(),
                ),
                Err(err) => (
                    {
                        eprintln!("Index page error: {}", err);
                        r#"<red>error</red>"#.to_string()
                    },
                    None,
                ),
            };
            let maps = if cfg!(feature = "map_viewer") {
                "<li><a href=\"maps\">maps</a></li>"
            } else {
                ""
            };
            let menu = max_rank
                .filter(|rank| *rank >= meta::Rank::GameMaster)
                .map_or(String::new(), |_| {
                    format!(
                        "<h1>Menu:</h1><ul>\
                         <li><a href=\"gm/clients\">clients</a></li>\
                         <li><a href=\"private/\">private</a></li>\
                         {}\
                         </ul>",
                        maps
                    )
                });
            format!(
                r#"User: {} <a href="/meta/logout">Logout</a>{}"#,
                name_string, menu
            )
        }
        None => r#"<a href="/meta/login">Login</a>"#.to_string(),
    };
    Ok(HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(body))
}

async fn go_web(data: web::Data<AppState>) -> HttpResponse {
    let url = data.config.host.web_url("/");
    HttpResponse::MovedPermanently()
        .append_header((actix_http::header::LOCATION, url))
        .finish()
}

/*
fn _info(
    req: HttpRequest,
    data: web::Data<AppState>,
) -> impl Future<Item = HttpResponse, Error = Error> {
    let crid = req
        .match_info()
        .get("crid")
        .and_then(|crid| crid.parse().ok());

    if let Some(crid) = crid {
        Either::A(
            data.get_ref()
                .critters_db
                .send(GetCritterInfo { id: crid })
                .from_err()
                .and_then(|res| match res {
                    Ok(Some(cr_info)) => Ok(format!("Your id: {:?}", cr_info.id).into()),
                    Ok(None) => Ok("I don't know about you!".into()),
                    Err(_) => Ok(HttpResponse::InternalServerError().into()),
                }),
        )
    } else {
        Either::B(fut_ok("Get out!".into()))
    }
}
*/
pub struct AppState {
    oauth: Option<Oauth2Client>,
    pub(crate) config: config::Config,
    pub(crate) mrhandy: Option<mrhandy::MrHandy>,
    pub(crate) sled_db: SledDb,
    critters_db: CrittersDb,
    pub(crate) bridge: bridge::Bridge,
    #[cfg(feature = "fo_data")]
    fo_data: Option<Arc<FoData>>,
    #[cfg(feature = "fo_proto_format")]
    items: Option<Arc<FoItems>>,
    reqwest: reqwest::Client,
    pub(crate) server_status: Mutex<bridge::Status>,
}

#[cfg(feature = "fo_proto_format")]
type FoItems = BTreeMap<u16, fo_proto_format::ProtoItem>;

pub struct AppDefinition {
    pub config: config::Config,
    pub db: sled::Db,
    #[cfg(feature = "fo_data")]
    fo_data: Option<FoData>,
    #[cfg(feature = "fo_proto_format")]
    items: Option<FoItems>,
}
impl AppDefinition {
    pub fn new(config: config::Config, db: sled::Db) -> Self {
        AppDefinition {
            config,
            db,
            #[cfg(feature = "fo_data")]
            fo_data: None,
            #[cfg(feature = "fo_proto_format")]
            items: None,
        }
    }

    #[cfg(feature = "fo_data")]
    pub fn with_data(mut self, data: FoData) -> Self {
        self.fo_data = Some(data);
        self
    }

    #[cfg(feature = "fo_proto_format")]
    pub fn with_items(mut self, items: FoItems) -> Self {
        self.items = Some(items);
        self
    }

    pub fn build(self) -> AppState {
        AppState::new(self)
    }
}

impl AppState {
    pub fn new(def: AppDefinition) -> Self {
        let AppDefinition { config, db, .. } = def;

        let critters_db = CrittersDb::new(config.paths.save_clients.clone());

        let sled_db = SledDb::new(db);
        let bridge = bridge::Bridge::new();

        let redirect = config.host.web_url("/meta/auth");
        let oauth = config
            .discord
            .as_ref()
            .map(|discord| oauth2_client(&discord.oauth2, redirect).expect("oauth client"));

        let reqwest = reqwest::Client::builder()
            // Following redirects opens the client up to SSRF vulnerabilities.
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(2))
            .use_rustls_tls()
            .build()
            .expect("Build reqwest client");

        Self {
            oauth,
            config,
            mrhandy: None,
            sled_db,
            critters_db,
            bridge,
            #[cfg(feature = "fo_data")]
            fo_data: def.fo_data.map(Arc::new),
            #[cfg(feature = "fo_proto_format")]
            items: def.items.map(Arc::new),
            reqwest,
            server_status: Mutex::new(bridge::Status::new()),
        }
    }

    #[cfg(feature = "fo_data")]
    pub fn fo_data(&self) -> &FoData {
        self.fo_data.as_ref().unwrap()
    }

    #[cfg(feature = "fo_proto_format")]
    pub fn fo_items(&self) -> &FoItems {
        self.items.as_ref().unwrap()
    }
}

pub type Oauth2Client = oauth2::Client<
    basic::BasicErrorResponse,
    basic::BasicTokenResponse,
    basic::BasicTokenIntrospectionResponse,
    oauth2::StandardRevocableToken,
    basic::BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

pub fn oauth2_client(
    config: &config::OAuth,
    redirect: String,
) -> Result<Oauth2Client, Box<dyn std::error::Error>> {
    use oauth2::{AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl};
    let client = oauth2::Client::new(
        ClientId::new(config.client_id.clone())
    ).set_client_secret(ClientSecret::new(config.secret.clone()))
    .set_auth_uri(AuthUrl::new("https://discordapp.com/api/oauth2/authorize".to_string())?)
    .set_token_uri(TokenUrl::new(
        "https://discordapp.com/api/oauth2/token".to_string(),
    )?)
    // Set the URL the user will be redirected to after the authorization process.
    .set_redirect_uri(RedirectUrl::new(redirect)?);
    Ok(client)
}

pub fn run(state: AppState) {
    println!("Starting actix-web server...");

    crate::templates::init();

    let sys = actix_rt::System::new();
    sys.block_on(run_async(state));
}

async fn run_async(mut state: AppState) {
    let mut serenity_client = if let Some(discord) = &state.config.discord {
        // TODO: Should we keep or join client fut?
        let (mrhandy, serenity_client) =
            mrhandy::init(&discord.bot.token, discord.main_guild_id).await;
        state.mrhandy = Some(mrhandy);
        Some(serenity_client)
    } else {
        None
    };

    let state = web::Data::new(state);

    let bridge_server = bridge::Bridge::start(state.clone().into_inner()).await;

    let web_server = HttpServer::new({
        let state = state.clone();
        move || {
            let cookies = actix_session::SessionMiddleware::builder(
                actix_session::storage::CookieSessionStore::default(),
                state.config.session.cookie_key(),
            )
            .cookie_secure(false)
            .cookie_name("meta-session".into())
            .build();
            let app = App::new()
                .app_data(state.clone())
                .wrap(middleware::Compress::default())
                .wrap(middleware::Logger::default())
                .wrap_fn(restrict_web)
                .wrap(cookies)
                .service(web::resource("/").route(web::get().to(index)))
                .service(
                    web::scope("/meta")
                        .service(web::resource("/login").route(web::get().to(meta::login)))
                        .service(web::resource("/logout").route(web::get().to(meta::logout)))
                        .service(web::resource("/auth").route(web::get().to(meta::auth))),
                )
                .service(
                    web::scope("/gm")
                        .wrap(restrict(meta::restrict_gm))
                        .service(web::resource("/clients").route(web::get().to(gm::clients)))
                        .service(
                            web::resource("/client/{client}").route(web::get().to(stats::gm_stats)),
                        ),
                )
                .service(
                    web::scope("/char/{id}")
                        .service(
                            web::scope("/edit")
                                .wrap(restrict(meta::restrict_ownership))
                                .service(
                                    web::resource("/avatar")
                                        .route(web::get().to(avatar::edit))
                                        .route(web::post().to(avatar::upload)),
                                ),
                        )
                        .service(
                            web::scope("/action")
                                .wrap(restrict(meta::restrict_ownership))
                                .service(
                                    web::resource("/start_game")
                                        .route(web::get().to(char_action::start_game)),
                                ),
                        )
                        .service(web::resource("/avatar").route(web::get().to(avatar::show))),
                )
                .service(actix_files::Files::new("/static", STATIC_PATH))
                .service({
                    let mut private = web::scope("/private")
                        .wrap(restrict(meta::restrict_gm))
                        .service(web::resource("/").route(web::get().to(list_privates)));
                    let name_path = state.config.paths.privates();
                    for (name, path) in name_path {
                        private = private.service(
                            actix_files::Files::new(&format!("/{}", name), path)
                                .show_files_listing()
                                .files_listing_renderer(dir::directory_listing),
                        );
                    }
                    private
                });
            #[cfg(feature = "map_viewer")]
            let app = app.service(
                web::scope("/maps")
                    .wrap(restrict(meta::restrict_gm))
                    //.service(web::resource("/tilemap").route(web::get().to(map_viewer::tilemap))),
                    .service(web::resource("/{path:.+}").route(web::get().to(map_viewer::view)))
                    .service(web::resource("").route(web::get().to(map_viewer::list))),
            );
            #[cfg(feature = "fo_data")]
            let app = app.service(
                web::resource("/data/{path:.+}")
                    .wrap(restrict(meta::restrict_gm))
                    .route(web::get().to(data::get)),
            );
            app
            //.service(
            //    web::resource("/{crid}").route(web::get().to_async(stats::gm_stats))
            //)
        }
    })
    .server_hostname(state.config.host.web.domain_port());

    let web_port = state.config.host.web_port();
    let web_server = if let Some(cert) = &state.config.host.web_tls {
        let tls_config = cert.server_config().expect("TLS server config");
        web_server.bind_rustls(("0.0.0.0", web_port), tls_config)
    } else {
        web_server.bind(("0.0.0.0", web_port))
    }
    .expect("Can't bind web server to port")
    .run();

    let file_server = HttpServer::new({
        let state = state.clone();
        move || {
            App::new()
                .app_data(state.clone())
                .wrap(middleware::Compress::default())
                .wrap(middleware::Logger::default())
                //.wrap_fn(restrict_files)
                .wrap(
                    middleware::DefaultHeaders::new()
                        .add(("Access-Control-Allow-Origin", state.config.host.web_url(""))),
                )
                .service(web::resource("/").route(web::get().to(go_web)))
                .service(actix_files::Files::new("/static", STATIC_PATH))
                .service(
                    web::scope("/char/{id}")
                        .service(web::resource("/avatar").route(web::get().to(avatar::show))),
                )
        }
    })
    .server_hostname(state.config.host.files.domain_port())
    .bind(("0.0.0.0", state.config.host.files_port()))
    .expect("Can't bind files server to port")
    .run();

    println!("Servers started!");

    let mut futs = vec![
        bridge_server.map_err(RuntimeError::Io).boxed(),
        web_server.map_err(RuntimeError::Io).boxed(),
        file_server.map_err(RuntimeError::Io).boxed(),
    ];
    if let Some(serenity_client) = serenity_client.as_mut() {
        futs.push(
            serenity_client
                .start()
                .map_err(RuntimeError::Serenity)
                .boxed(),
        );
        let status_updater = status_updater(state);
        futs.push(status_updater.boxed());
    }
    let (res, _, _) = futures::future::select_all(futs).await;
    println!("Stopping... Result: {:?}", res);
}

async fn status_updater(state: web::Data<AppState>) -> Result<(), RuntimeError> {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        let mut server_status = state.server_status.lock().await;
        let mrhandy = state.mrhandy.as_ref().expect("MrHandy");
        server_status.new_status(mrhandy).await;
        //.map_err(RuntimeError::Serenity)?;
    }
}

#[derive(Debug)]
enum RuntimeError {
    Io(std::io::Error),
    Serenity(mrhandy::serenity::Error),
}

fn req_host(req: &ServiceRequest) -> Option<&str> {
    let mut host = req.uri().host();
    if host.is_none() {
        host = req
            .headers()
            .get(actix_http::header::HOST)
            .and_then(|header| header.to_str().ok())
            .and_then(|host_port| host_port.split(':').next());
    }
    host
}

fn restrict_web<
    B: MessageBody,
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
>(
    req: ServiceRequest,
    srv: &S,
) -> impl Future<Output = Result<ServiceResponse<B>, actix_web::Error>> {
    let data: &web::Data<AppState> = req.app_data().expect("AppData");
    let host = req_host(&req);
    let check = host.map_or(false, |host| host == data.config.host.web.domain);
    let fut = srv.call(req);
    async move {
        if check {
            fut.await
        } else {
            Err(meta::access_denied("Wrong domain name")())?
        }
    }
}

fn _restrict_files<
    B: MessageBody,
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
>(
    req: ServiceRequest,
    srv: &S,
) -> impl Future<Output = Result<ServiceResponse<B>, actix_web::Error>> {
    let data: &web::Data<AppState> = req.app_data().expect("AppData");
    let host = req_host(&req);
    let check = host.map_or(false, |host| host == data.config.host.files.domain);
    let fut = srv.call(req);
    async move {
        if check {
            fut.await
        } else {
            Err(meta::access_denied("Wrong domain name")())?
        }
    }
}

async fn list_privates(data: web::Data<AppState>) -> HttpResponse {
    let name_path = data.config.paths.privates();
    let body: String = name_path
        .keys()
        .map(|name| format!(r#"<p><a href="{0}">{0}</a></p>"#, name))
        .collect();
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(body)
}

fn internal_error<D: std::fmt::Debug>(err: D) -> actix_web::Error {
    let text = format!("Internal error: {:?}", err);
    InternalError::from_response(
        text.clone(),
        HttpResponse::BadRequest()
            .content_type("text/plain; charset=utf-8")
            .body(text),
    )
    .into()
}
