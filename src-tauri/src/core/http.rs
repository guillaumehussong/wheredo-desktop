//! Thin reqwest wrappers mirroring the macOS `HTTP.swift` helpers.

use std::collections::HashMap;

pub const USER_AGENT: &str = concat!("wheredo/0.1 (", env!("CARGO_PKG_NAME"), " desktop)");

#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("reqwest client")
}

pub async fn post_form(
    url: &str,
    fields: &HashMap<&str, &str>,
) -> Result<HttpResponse, reqwest::Error> {
    let resp = client()
        .post(url)
        .header("Accept", "application/json")
        .form(fields)
        .send()
        .await?;
    let status = resp.status().as_u16();
    let body = resp.bytes().await?.to_vec();
    Ok(HttpResponse { status, body })
}

pub async fn post_json(
    url: &str,
    body: &serde_json::Value,
    bearer: Option<&str>,
) -> Result<HttpResponse, reqwest::Error> {
    let mut req = client()
        .post(url)
        .header("Accept", "application/json")
        .json(body);
    if let Some(token) = bearer {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let bytes = resp.bytes().await?.to_vec();
    Ok(HttpResponse { status, body: bytes })
}

#[allow(dead_code)] // parity with the macOS HTTP helpers; used by future --models
pub async fn get_json(url: &str, bearer: Option<&str>) -> Result<HttpResponse, reqwest::Error> {
    let mut req = client().get(url).header("Accept", "application/json");
    if let Some(token) = bearer {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let bytes = resp.bytes().await?.to_vec();
    Ok(HttpResponse { status, body: bytes })
}

/// POST /v1/stt — multipart upload (file must be the last field).
pub async fn post_stt(
    api_base: &str,
    wav: Vec<u8>,
    language: &str,
    bearer: &str,
) -> Result<HttpResponse, reqwest::Error> {
    let file_part = reqwest::multipart::Part::bytes(wav)
        .file_name("recording.wav")
        .mime_str("audio/wav")
        .expect("valid mime");
    let form = reqwest::multipart::Form::new()
        .text("language", language.to_string())
        .part("file", file_part);

    let resp = client()
        .post(format!("{api_base}/stt"))
        .header("Accept", "application/json")
        .bearer_auth(bearer)
        .multipart(form)
        .send()
        .await?;
    let status = resp.status().as_u16();
    let body = resp.bytes().await?.to_vec();
    Ok(HttpResponse { status, body })
}
