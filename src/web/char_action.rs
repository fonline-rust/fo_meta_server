use actix_web::{web, Error, HttpResponse};

use crate::bridge::MsgOut;

pub async fn start_game(
    path: web::Path<u32>,
    data: web::Data<super::AppState>,
) -> Result<HttpResponse, Error> {
    Ok(
        if data
            .bridge
            .get_sender()
            .and_then(|mut sender| sender.try_send(MsgOut::StartGame { player_id: *path }).ok())
            .is_some()
        {
            HttpResponse::Ok().body("Request to start game sent.")
        } else {
            HttpResponse::InternalServerError().body("Lost connection to game server.")
        },
    )
}
