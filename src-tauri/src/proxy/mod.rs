//! Local Proxy 模块（ADR-0020 §4 / plan3）。
//!
//! 提供 Listener, Route, CredentialPool, ProxyEngine, Usage 与 HTTP Server 运行时。

pub mod codec;
pub mod engine;
pub mod listener;
pub mod model;
pub mod native;
pub mod routing;
pub mod store;

pub use codec::{
    decode_inbound_request, encode_upstream_request, CanonicalEvent, CanonicalMessage,
    CanonicalRequest, CanonicalTool, CanonicalUsage, InboundCompletionEncoder,
    InboundStreamEncoder, ProtocolKind,
};
pub use engine::{EngineCall, EngineError, EngineOutcome, ProxyEngine};
pub use listener::ProxyRuntime;
pub use model::{
    CredentialHealthInfo, CredentialSelector, PoolPolicy, PortMode, ProxyEndpointInfo,
    ProxyRuntimeStatus, ProxySettings, ProxyStatus, ProxyStatusDTO, ProxyUsageRecord, Route,
    RouteDefinition, RouteTarget,
};
pub use native::NativeProxyEngine;
pub use routing::{ResolvedRouteTarget, RouteResolver};
