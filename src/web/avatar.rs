use std::io::Cursor;

use actix_web::{error::BlockingError, web, HttpResponse};
use arrayvec::ArrayVec;
use futures::{
    future::{err as fut_err, Either},
    Future, FutureExt, TryFutureExt,
};
use serde::{Deserialize, Serialize};

use crate::{
    bridge,
    database::{CharTrunk, Leaf, Root, VersionedError},
    templates,
    utils::blocking,
};

// size of square image in pixels, 128 means 128x128
const IMAGE_SIZE: u32 = 128;
const AUTH_LEN: usize = 12;
const AUTH_HEX_LEN: usize = AUTH_LEN * 2;

#[derive(Deserialize)]
pub struct VersionSecret {
    ver: Option<u32>,
    secret: Option<u32>,
}

#[derive(Deserialize)]
pub struct Auth {
    auth: Option<String>,
}

// ===== Check auth =====

pub type AuthVec = ArrayVec<u8, AUTH_LEN>;
pub fn parse_auth(auth: &Auth) -> Option<(AuthVec, String)> {
    let str: &str = auth.auth.as_ref()?.as_str();
    if str.len() != AUTH_HEX_LEN {
        return None;
    }
    let auth_string = str.to_uppercase();
    dbg!(&auth_string);
    let mut arr = AuthVec::new();
    let mut cur = auth_string.as_str();
    while !cur.is_empty() {
        let (chunk, rest) = cur.split_at(std::cmp::min(2, cur.len()));
        let res = u8::from_str_radix(chunk, 16).ok()?;
        arr.push(res);
        cur = rest;
    }
    if !arr.is_full() {
        return None;
    }
    Some((arr, auth_string))
}

// ===== Avatar editor =====

#[derive(Debug, Serialize)]
struct AvatarEditor {
    char_id: u32,
}

pub async fn edit(
    path: web::Path<u32>,
    data: web::Data<super::AppState>,
) -> actix_web::Result<HttpResponse> {
    let char_id = *path;

    let res = blocking(move || {
        templates::render(
            "edit_avatar.html",
            &AvatarEditor { char_id },
            templates::RenderConfig {
                host: Some(&data.config.host),
            },
        )
        .map_err(AvatarUploadError::Template)
    })
    .await;
    Ok(match res {
        Err(AvatarUploadError::Template(err)) => {
            eprintln!("AvatarEditor template error: {:#?}", err);
            HttpResponse::InternalServerError().finish()
        }
        Err(_) => HttpResponse::Forbidden().finish(),
        Ok(body) => HttpResponse::Ok().content_type("text/html").body(body),
    })
}

// ===== Upload avatar =====

pub fn upload(
    path: web::Path<u32>,
    data: web::Data<super::AppState>,
    payload: web::Bytes,
) -> impl Future<Output = Result<HttpResponse, AvatarUploadError>> {
    const MIN_LEN: usize = 16;
    const MAX_LEN: usize = 128 * 1024;
    const PREFIX_LEN: usize = 22;
    const PREFIX: &[u8; PREFIX_LEN] = b"data:image/png;base64,";

    if payload.len() <= PREFIX_LEN || !payload.starts_with(PREFIX) {
        return Either::Left(fut_err(AvatarUploadError::DataUrl));
    }

    let data_len = payload.len() - PREFIX_LEN;
    if !(MIN_LEN..=MAX_LEN).contains(&data_len) {
        return Either::Left(fut_err(AvatarUploadError::DataLength(data_len)));
    }

    let char_id = *path;
    let root = data.sled_db.root.clone();
    let sender = data.bridge.get_sender();
    Either::Right(
        blocking(move || {
            let data = &payload[PREFIX_LEN..];
            save_image(&root, char_id, data)
        })
        .map(move |res| res.and_then(|leaf| update_char_leaf(sender, char_id, leaf)))
        .map_ok(|_| HttpResponse::NoContent().finish()),
    )
}

fn save_image(root: &Root, char_id: u32, data: &[u8]) -> Result<Leaf<()>, AvatarUploadError> {
    use image::{DynamicImage, ImageFormat};

    let instant = std::time::Instant::now();
    let decoded =
        base64::decode_config(data, base64::STANDARD).map_err(AvatarUploadError::Base64)?;
    println!("Decoded in {:?}", instant.elapsed());
    let instant2 = std::time::Instant::now();
    let image = image::load_from_memory_with_format(&decoded, ImageFormat::Png)
        .map_err(AvatarUploadError::ImageLoad)?;
    println!("Loaded in {:?}", instant2.elapsed());
    let instant2 = std::time::Instant::now();
    if image.width() != IMAGE_SIZE || image.height() != IMAGE_SIZE {
        return Err(AvatarUploadError::ImageSize(image.width(), image.height()));
    }
    let new_image = DynamicImage::ImageRgb8(image.to_rgb8());

    let mut buffer = decoded;
    buffer.clear();
    let mut cursor = Cursor::new(buffer);
    new_image
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(AvatarUploadError::ImageWrite)?;
    println!("Writed in {:?}", instant2.elapsed());
    let instant2 = std::time::Instant::now();

    let leaf = root
        .trunk(char_id, None, CharTrunk::default())
        .set_image(cursor.into_inner())
        .map_err(AvatarUploadError::SledVersioned)?;
    println!("Saved to db in {:?}", instant2.elapsed());

    println!("Fully saved in {:?}", instant.elapsed());

    Ok(leaf)
}

fn update_char_leaf(
    sender: Option<bridge::MsgOutSender>,
    id: u32,
    leaf: Leaf<()>,
) -> Result<(), AvatarUploadError> {
    match (sender, leaf) {
        (
            Some(mut sender),
            Leaf {
                ver,
                secret: Some(secret),
                ..
            },
        ) => sender
            .try_send(bridge::MsgOut::UpdateCharLeaf { id, ver, secret })
            .map_err(|_| AvatarUploadError::FuturesSyncSend),
        _ => Ok(()),
    }
}

// ===== Show avatar =====

pub async fn show(
    path: web::Path<u32>,
    query: web::Query<VersionSecret>,
    data: web::Data<super::AppState>,
) -> actix_web::Result<HttpResponse> {
    let VersionSecret { ver, secret } = *query;

    if secret.is_none() {
        return Ok(HttpResponse::Forbidden().finish());
    }

    let root = data.sled_db.root.clone();
    let res = blocking(move || {
        let instant = std::time::Instant::now();
        let leaf = root
            .trunk(*path, ver, CharTrunk::default())
            .get_image(secret)?;
        println!("Getting image, completed in {:?}", instant.elapsed());
        Ok(leaf)
    })
    .await;
    Ok(match res {
        Ok(image) => HttpResponse::Ok()
            .append_header(("q-ver", image.ver as u64))
            .append_header(("q-length", image.data.len()))
            .content_type("image/png")
            .body(bytes::Bytes::copy_from_slice(image.data.as_ref())),
        Err(VersionedError::NotFound) => HttpResponse::NotFound().finish(),
        Err(err) => HttpResponse::InternalServerError().body(format!("Error: {:?}", err)),
    })
}

// ===== AvatarUploadError =====

#[derive(Debug)]
pub enum AvatarUploadError {
    DataUrl,
    DataLength(usize),
    Blocking,
    Base64(base64::DecodeError),
    ImageLoad(image::ImageError),
    ImageSize(u32, u32),
    ImageWrite(image::ImageError),
    SledVersioned(VersionedError),
    FuturesSyncSend,
    Template(templates::TemplatesError),
}

impl From<BlockingError> for AvatarUploadError {
    fn from(_err: BlockingError) -> Self {
        AvatarUploadError::Blocking
    }
}

impl std::fmt::Display for AvatarUploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl actix_web::error::ResponseError for AvatarUploadError {
    fn error_response(&self) -> HttpResponse {
        log::warn!("{:?}", self);

        use actix_web::http::StatusCode;
        HttpResponse::new(StatusCode::INTERNAL_SERVER_ERROR)
    }
    // TODO: Investigate error rendering
    /*
    /// Constructs an error response
    fn render_response(&self) -> HttpResponse {


        use actix_web::{http::{header, StatusCode}, body::Body};

        let mut resp = self.error_response();
        let mut buf = web::BytesMut::new();
        let _ = write!(Writer(&mut buf), "{}", self);
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/plain"),
        );
        resp.set_body(Body::from(buf))
    }*/
}
