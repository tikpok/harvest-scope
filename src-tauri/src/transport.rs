use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{header::LOCATION, Response};
use serde_json::Value;
use url::Url;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy)]
pub enum ResponseBudget {
    Health,
    Document,
    Collection,
}

impl ResponseBudget {
    pub const fn bytes(self) -> usize {
        match self {
            Self::Health => 64 * 1024,
            Self::Document => 1024 * 1024,
            Self::Collection => 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone)]
pub struct NodeTransport {
    client: reqwest::Client,
}

impl NodeTransport {
    pub fn new() -> AppResult<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(2)
            .user_agent("Harvest-Scope/1")
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { client })
    }

    pub async fn get_json(&self, url: &str, budget: ResponseBudget) -> AppResult<Value> {
        let mut current = Url::parse(url)?;
        for redirect_count in 0..=3 {
            let response = self.client.get(current.clone()).send().await?;
            if response.status().is_redirection() {
                if redirect_count == 3 {
                    return Err(AppError::RedirectRejected(
                        "リダイレクト回数が3回を超えました".into(),
                    ));
                }
                current = redirect_target(&current, &response)?;
                continue;
            }
            ensure_success(&response)?;
            return read_json(response, budget).await;
        }
        unreachable!("redirect loop always returns or continues within its fixed bound")
    }

    pub async fn post_json(
        &self,
        url: &str,
        body: &Value,
        budget: ResponseBudget,
    ) -> AppResult<Value> {
        let response = self.client.post(url).json(body).send().await?;
        if response.status().is_redirection() {
            return Err(AppError::RedirectRejected(format!(
                "POST先がHTTP {}を返しました",
                response.status()
            )));
        }
        ensure_success(&response)?;
        read_json(response, budget).await
    }
}

fn ensure_success(response: &Response) -> AppResult<()> {
    if response.status().is_success() {
        Ok(())
    } else {
        Err(AppError::Network(format!("HTTP {}", response.status())))
    }
}

fn redirect_target(current: &Url, response: &Response) -> AppResult<Url> {
    let location = response
        .headers()
        .get(LOCATION)
        .ok_or_else(|| AppError::RedirectRejected("Locationヘッダーがありません".into()))?
        .to_str()
        .map_err(|_| AppError::RedirectRejected("Locationヘッダーが不正です".into()))?;
    let target = current
        .join(location)
        .map_err(|error| AppError::RedirectRejected(error.to_string()))?;
    if !target.username().is_empty() || target.password().is_some() {
        return Err(AppError::RedirectRejected(
            "資格情報を含むURLには移動できません".into(),
        ));
    }
    if !same_origin(current, &target) {
        return Err(AppError::RedirectRejected(format!(
            "別originへの移動は許可されません: {target}"
        )));
    }
    Ok(target)
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left
            .host_str()
            .zip(right.host_str())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
        && left.port_or_known_default() == right.port_or_known_default()
}

async fn read_json(response: Response, budget: ResponseBudget) -> AppResult<Value> {
    let limit = budget.bytes();
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(AppError::ResponseTooLarge { limit });
    }
    let mut bytes =
        Vec::with_capacity(response.content_length().unwrap_or(0).min(limit as u64) as usize);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(AppError::ResponseTooLarge { limit });
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    async fn spawn_server<F>(max_requests: usize, handler: F) -> (String, Arc<AtomicUsize>)
    where
        F: Fn(&str) -> String + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let request_count = requests.clone();
        let handler = Arc::new(handler);
        tokio::spawn(async move {
            for _ in 0..max_requests {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut request = vec![0u8; 8192];
                let read = stream.read(&mut request).await.unwrap_or_default();
                request_count.fetch_add(1, Ordering::SeqCst);
                let request = String::from_utf8_lossy(&request[..read]);
                let response = handler(&request);
                stream.write_all(response.as_bytes()).await.unwrap();
                let _ = stream.shutdown().await;
            }
        });
        (format!("http://{address}"), requests)
    }

    #[tokio::test]
    async fn follows_only_same_origin_get_redirects() {
        let (base, requests) = spawn_server(2, |request| {
            if request.starts_with("GET /start ") {
                "HTTP/1.1 302 Found\r\nLocation: /ok\r\nContent-Length: 0\r\n\r\n".into()
            } else {
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}".into()
            }
        }).await;
        let value = NodeTransport::new()
            .unwrap()
            .get_json(&format!("{base}/start"), ResponseBudget::Health)
            .await
            .unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(requests.load(Ordering::SeqCst), 2);

        let (base, _) = spawn_server(1, |_| {
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/private\r\nContent-Length: 0\r\n\r\n".into()
        }).await;
        let error = NodeTransport::new()
            .unwrap()
            .get_json(&format!("{base}/start"), ResponseBudget::Health)
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::RedirectRejected(_)));
    }

    #[tokio::test]
    async fn rejects_post_redirect_without_replaying_body() {
        let (base, requests) = spawn_server(1, |_| {
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: /replay\r\nContent-Length: 0\r\n\r\n"
                .into()
        })
        .await;
        let error = NodeTransport::new()
            .unwrap()
            .post_json(
                &format!("{base}/accounts"),
                &serde_json::json!({"addresses": ["A"]}),
                ResponseBudget::Collection,
            )
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::RedirectRejected(_)));
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rejects_content_length_and_chunked_bodies_over_budget() {
        let oversized = ResponseBudget::Health.bytes() + 1;
        let (base, _) = spawn_server(1, move |_| {
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {oversized}\r\n\r\n")
        }).await;
        let error = NodeTransport::new()
            .unwrap()
            .get_json(&base, ResponseBudget::Health)
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::ResponseTooLarge { .. }));

        let body = format!("\"{}\"", "x".repeat(ResponseBudget::Health.bytes()));
        let chunk_size = body.len();
        let (base, _) = spawn_server(1, move |_| {
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{chunk_size:X}\r\n{body}\r\n0\r\n\r\n")
        }).await;
        let error = NodeTransport::new()
            .unwrap()
            .get_json(&base, ResponseBudget::Health)
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::ResponseTooLarge { .. }));
    }
}
