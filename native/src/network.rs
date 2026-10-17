use futures::{future::BoxFuture, io::AsyncReadExt as _, FutureExt as _};
use gpui::http_client::{self, HttpClient};
use std::time::Duration;

/// HTTP client used only by explicit, host-allowlisted `data-fetch` controls.
#[derive(Debug)]
pub struct NativeHttpClient {
    inner: zed_reqwest::Client,
}

impl NativeHttpClient {
    pub fn new() -> Self {
        let inner = zed_reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .redirect_policy(zed_reqwest::redirect::Policy::none())
            .build()
            .expect("HTTP client configuration is valid");
        Self { inner }
    }
}

impl HttpClient for NativeHttpClient {
    fn type_name(&self) -> &'static str {
        "TSGPUI allowlisted HTTP client"
    }

    fn user_agent(&self) -> Option<&http_client::http::HeaderValue> {
        None
    }

    fn send(
        &self,
        request: http_client::Request<http_client::AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<http_client::Response<http_client::AsyncBody>>> {
        let client = self.inner.clone();
        async move {
            let (parts, body) = request.into_parts();
            let mut request_bytes = Vec::new();
            body.take(1024 * 1024 + 1).read_to_end(&mut request_bytes).await?;
            if request_bytes.len() > 1024 * 1024 {
                anyhow::bail!("TSGPUI request body exceeds 1 MiB");
            }
            let mut request_builder = client.request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .version(parts.version);
            request_builder = request_builder.body(request_bytes);
            let request = request_builder.build()?;
            let mut response = client.execute(request).await?;
            let status = response.status();
            let version = response.version();
            let headers = response.headers().clone();
            let mut response_bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if response_bytes.len() + chunk.len() > 1024 * 1024 {
                    anyhow::bail!("TSGPUI response body exceeds 1 MiB");
                }
                response_bytes.extend_from_slice(&chunk);
            }
            let mut response_builder = http_client::Response::builder().status(status).version(version);
            for (name, value) in &headers { response_builder = response_builder.header(name, value); }
            Ok(response_builder.body(http_client::AsyncBody::from_bytes(response_bytes.into()))?)
        }
        .boxed()
    }

    fn proxy(&self) -> Option<&http_client::Url> {
        None
    }
}
