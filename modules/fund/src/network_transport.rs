//! Bounded blocking transport shared by quote, information and notification owners.
use super::{current, Context};
use serde_json::Value;
use std::{
    io::Read,
    sync::{Arc, OnceLock},
    time::Duration,
};
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
#[derive(Debug)]
struct PublicResolver;
impl Resolver for PublicResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let addresses = DefaultResolver::default().resolve(uri, config, timeout)?;
        if addresses
            .iter()
            .any(|a| !crate::channels::public_ip(a.ip()))
        {
            return Err(ureq::Error::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "blocked_target",
            )));
        }
        Ok(addresses)
    }
}
fn build(proxy: Option<ureq::Proxy>) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .proxy(proxy)
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_resolve(Some(Duration::from_secs(5)))
        .user_agent("natives-fund/1.0")
        .build();
    ureq::Agent::with_parts(
        config,
        ureq::unversioned::transport::DefaultConnector::default(),
        PublicResolver,
    )
}
pub(super) fn agent(context: &Context, config: &Value) -> Result<ureq::Agent, Error> {
    super::validate(config).map_err(|_| Error::Network)?;
    if config["mode"] == "direct" {
        return Ok(direct());
    }
    let url = url::Url::parse(config["url"].as_str().ok_or(Error::Network)?)
        .map_err(|_| Error::Network)?;
    let mut builder = ureq::Proxy::builder(ureq::ProxyProtocol::Https)
        .host(url.host_str().ok_or(Error::Network)?)
        .port(443)
        .resolve_target(true);
    let username = config["username"].as_str().ok_or(Error::Network)?;
    if !username.is_empty() {
        let password = context.password().map_err(|_| Error::Network)?;
        builder = builder.username(username).password(&password);
    }
    Ok(build(Some(builder.build().map_err(|_| Error::Network)?)))
}
fn direct() -> ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| build(None)).clone()
}
pub(crate) struct Client;
pub(crate) fn client() -> &'static Client {
    static CLIENT: Client = Client;
    &CLIENT
}
impl Client {
    pub fn get(&self, url: &str) -> Request {
        Request::new(url, false)
    }
    pub fn post(&self, url: &str) -> Request {
        Request::new(url, true)
    }
}
pub(crate) struct Request {
    url: String,
    post: bool,
    headers: Vec<(String, String)>,
    timeout: Duration,
    context: Option<Arc<Context>>,
}
impl Request {
    fn new(url: &str, post: bool) -> Self {
        Self {
            url: url.to_owned(),
            post,
            headers: vec![],
            timeout: Duration::from_secs(10),
            context: current(),
        }
    }
    pub fn set(mut self, key: &str, value: &str) -> Self {
        self.headers.push((key.to_owned(), value.to_owned()));
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.min(Duration::from_secs(10));
        self
    }
    pub fn call(self) -> Result<Response, Error> {
        self.execute(None)
    }
    pub fn send_string(self, body: &str) -> Result<Response, Error> {
        self.execute(Some(body))
    }
    fn execute(self, body: Option<&str>) -> Result<Response, Error> {
        let timeout =
            super::remaining().map_or(self.timeout, |remaining| remaining.min(self.timeout));
        if timeout.is_zero() {
            return Err(Error::Network);
        }
        if !crate::channels::host_allowed(&self.url) {
            return Err(Error::Network);
        }
        let (client, version) = if let Some(ctx) = &self.context {
            let mut config = ctx.config().map_err(|_| Error::Network)?;
            let version = config
                .as_object_mut()
                .ok_or(Error::Network)?
                .remove("version")
                .ok_or(Error::Network)?;
            if config["mode"] == "https_proxy"
                && config["username"].as_str().is_some_and(|s| !s.is_empty())
            {
                ctx.verify_active_credentials()
                    .map_err(|_| Error::Network)?;
            }
            (agent(ctx, &config)?, Some(version))
        } else {
            (direct(), None)
        };
        let response = if self.post {
            let mut request = client.post(&self.url);
            for (k, v) in &self.headers {
                request = request.header(k, v);
            }
            request
                .config()
                .timeout_global(Some(timeout))
                .build()
                .send(body.unwrap_or_default())
        } else {
            let mut request = client.get(&self.url);
            for (k, v) in &self.headers {
                request = request.header(k, v);
            }
            request
                .config()
                .timeout_global(Some(timeout))
                .build()
                .call()
        }
        .map_err(|_| Error::Network)?;
        if let Some(ctx) = &self.context {
            if ctx.config().map_err(|_| Error::Network)?["version"]
                != version.unwrap_or(Value::Null)
            {
                return Err(Error::Network);
            }
        }
        let response = Response(response);
        let code = response.status();
        if code >= 400 {
            Err(Error::Status(code, response))
        } else {
            Ok(response)
        }
    }
}
pub(crate) struct Response(ureq::http::Response<ureq::Body>);
impl Response {
    pub fn status(&self) -> u16 {
        self.0.status().as_u16()
    }
    pub fn into_reader(self) -> impl Read {
        self.0.into_body().into_reader().take(2 * 1024 * 1024 + 1)
    }
    pub fn into_string(self) -> Result<String, std::io::Error> {
        let mut bytes = vec![];
        self.into_reader().read_to_end(&mut bytes)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(std::io::Error::other("response_too_large"));
        }
        String::from_utf8(bytes).map_err(|_| std::io::Error::other("response_encoding"))
    }
    pub fn into_json<T: serde::de::DeserializeOwned>(self) -> Result<T, std::io::Error> {
        serde_json::from_str(&self.into_string()?)
            .map_err(|_| std::io::Error::other("response_json"))
    }
}
pub(crate) enum Error {
    Status(u16, Response),
    Network,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Status(code, _) => write!(f, "upstream_status_{code}"),
            Self::Network => f.write_str("network_failed_or_delivery_unknown"),
        }
    }
}
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
