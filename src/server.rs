use gotham::handler::HandlerResult;
use gotham::handler::IntoResponse;

use gotham::helpers::http::response::create_empty_response;
use gotham::helpers::http::response::create_response;
use gotham::hyper::{body, header, Body, HeaderMap, Response, StatusCode};
use gotham::middleware::state::StateMiddleware;
use gotham::pipeline::single::single_pipeline;
use gotham::pipeline::single_middleware;
use gotham::router::{
    builder::build_router, builder::DefineSingleRoute, builder::DrawRoutes, Router,
};
use gotham::state::{FromState, State};
use mime::Mime;

use gotham_derive::StateData;

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;

use log::*;

use crate::config::Config;
use crate::tunnel::{BodyError, RemoteSentryInstance};

// 1 MB max body
pub const MAX_CONTENT_SIZE: u16 = 1_000;

#[derive(Debug, StateData, Clone)]
struct TunnelConfig {
    inner: Arc<Config>,
}

fn parse_body(body: String) -> Result<RemoteSentryInstance, BodyError> {
    RemoteSentryInstance::try_new_from_body(body)
}

#[derive(Debug)]
pub enum HeaderError {
    MissingContentLength,
    ContentIsTooBig,
    CouldNotParseContentLength,
}

impl Error for HeaderError {}

impl Display for HeaderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            HeaderError::MissingContentLength => f.write_str("Missing content length header"),
            HeaderError::ContentIsTooBig => f.write_str("Content length too big"),
            HeaderError::CouldNotParseContentLength => {
                f.write_str("could not parse content length header")
            }
        }
    }
}

impl IntoResponse for HeaderError {
    fn into_response(self, state: &State) -> Response<Body> {
        warn!("{}", self);
        let mime = "application/json".parse::<Mime>().unwrap();
        create_response(state, StatusCode::BAD_REQUEST, mime, format!("{}", self))
    }
}

fn check_content_length(headers: &HeaderMap) -> Result<(), HeaderError> {
    if let Some(content_length_value) = headers.get(header::CONTENT_LENGTH) {
        let content_length = u16::from_str(
            content_length_value
                .to_str()
                .map_err(|_| HeaderError::CouldNotParseContentLength)?,
        )
        .map_err(|_| HeaderError::CouldNotParseContentLength)?;
        if content_length > MAX_CONTENT_SIZE {
            return Err(HeaderError::ContentIsTooBig);
        } else {
            return Ok(());
        }
    }
    Err(HeaderError::MissingContentLength)
}

/// Extracts the elements of the POST request and prints them
async fn post_tunnel_handler(mut state: State) -> HandlerResult {
    let headers = HeaderMap::take_from(&mut state);

    match check_content_length(&headers) {
        Ok(_) => {
            let full_body = body::to_bytes(Body::take_from(&mut state)).await;
            match full_body {
                Ok(valid_body) => {
                    let body_content = String::from_utf8(valid_body.to_vec()).unwrap();
                    match parse_body(body_content) {
                        Ok(sentry_instance) => {
                            let config = TunnelConfig::borrow_from(&state);
                            let host = &config.inner.remote_host;
                            if config
                                .inner
                                .project_id_is_allowed(&sentry_instance.project_id)
                            {
                                if let Err(e) = sentry_instance.forward(host).await {
                                    error!("{} - Host = {}", e, host);
                                }
                                let res = create_empty_response(&state, StatusCode::OK);
                                Ok((state, res))
                            } else {
                                let res = BodyError::InvalidProjectId.into_response(&state);
                                Ok((state, res))
                            }
                        }
                        Err(e) => {
                            let res = e.into_response(&state);
                            Ok((state, res))
                        }
                    }
                }
                Err(e) => Err((state, e.into())),
            }
        }
        Err(e) => {
            let res = e.into_response(&state);
            Ok((state, res))
        }
    }
}

pub fn router(path: &str, config: Config) -> Router {
    let middleware = StateMiddleware::new(TunnelConfig {
        inner: Arc::new(config),
    });
    let pipeline = single_middleware(middleware);
    let (chain, pipelines) = single_pipeline(pipeline);

    build_router(chain, pipelines, |route| {
        route.post(path).to_async(post_tunnel_handler);
    })
}
