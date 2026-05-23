///Shorthand for `OwnedView<GetGraphRequestView<'static>>`.
pub type OwnedGetGraphRequestView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetGraphRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetGraphResponseView<'static>>`.
pub type OwnedGetGraphResponseView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetGraphResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetRepoRequestView<'static>>`.
pub type OwnedGetRepoRequestView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetRepoRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetRepoResponseView<'static>>`.
pub type OwnedGetRepoResponseView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetRepoResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<ListTriggersRequestView<'static>>`.
pub type OwnedListTriggersRequestView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::ListTriggersRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<ListTriggersResponseView<'static>>`.
pub type OwnedListTriggersResponseView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::ListTriggersResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<SearchRequestView<'static>>`.
pub type OwnedSearchRequestView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::SearchRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<SearchResponseView<'static>>`.
pub type OwnedSearchResponseView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::SearchResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetEventImpactRequestView<'static>>`.
pub type OwnedGetEventImpactRequestView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetEventImpactRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetEventImpactResponseView<'static>>`.
pub type OwnedGetEventImpactResponseView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetEventImpactResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetNodeRequestView<'static>>`.
pub type OwnedGetNodeRequestView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetNodeRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetNodeResponseView<'static>>`.
pub type OwnedGetNodeResponseView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetNodeResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetImpactRequestView<'static>>`.
pub type OwnedGetImpactRequestView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetImpactRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetImpactResponseView<'static>>`.
pub type OwnedGetImpactResponseView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetImpactResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<TraceRequestView<'static>>`.
pub type OwnedTraceRequestView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::TraceRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<TraceResponseView<'static>>`.
pub type OwnedTraceResponseView = ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::TraceResponseView<
        'static,
    >,
>;
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::GetGraphResponse,
>
for crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetGraphResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::GetGraphResponse,
>
for ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetGraphResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(&**self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::GetRepoResponse,
>
for crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetRepoResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::GetRepoResponse,
>
for ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetRepoResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(&**self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::ListTriggersResponse,
>
for crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::ListTriggersResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::ListTriggersResponse,
>
for ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::ListTriggersResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(&**self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::SearchResponse,
>
for crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::SearchResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::SearchResponse,
>
for ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::SearchResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(&**self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::GetEventImpactResponse,
>
for crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetEventImpactResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::GetEventImpactResponse,
>
for ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetEventImpactResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(&**self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::GetNodeResponse,
>
for crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetNodeResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::GetNodeResponse,
>
for ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetNodeResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(&**self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::GetImpactResponse,
>
for crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetImpactResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::GetImpactResponse,
>
for ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetImpactResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(&**self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::TraceResponse,
>
for crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::TraceResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::cli::render::browse::proto::ravelact::browse::v1::TraceResponse,
>
for ::buffa::view::OwnedView<
    crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::TraceResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(&**self, codec)
    }
}
/// Full service name for this service.
pub const BROWSE_SERVICE_SERVICE_NAME: &str = "ravelact.browse.v1.BrowseService";
/// Static [`Spec`](::connectrpc::Spec) for the server-side `GetGraph` RPC.
///
/// The dispatcher surfaces this on
/// [`RequestContext::spec`](::connectrpc::RequestContext::spec).
pub const BROWSE_SERVICE_GET_GRAPH_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/ravelact.browse.v1.BrowseService/GetGraph",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the server-side `GetRepo` RPC.
///
/// The dispatcher surfaces this on
/// [`RequestContext::spec`](::connectrpc::RequestContext::spec).
pub const BROWSE_SERVICE_GET_REPO_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/ravelact.browse.v1.BrowseService/GetRepo",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the server-side `ListTriggers` RPC.
///
/// The dispatcher surfaces this on
/// [`RequestContext::spec`](::connectrpc::RequestContext::spec).
pub const BROWSE_SERVICE_LIST_TRIGGERS_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/ravelact.browse.v1.BrowseService/ListTriggers",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the server-side `Search` RPC.
///
/// The dispatcher surfaces this on
/// [`RequestContext::spec`](::connectrpc::RequestContext::spec).
pub const BROWSE_SERVICE_SEARCH_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/ravelact.browse.v1.BrowseService/Search",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the server-side `GetEventImpact` RPC.
///
/// The dispatcher surfaces this on
/// [`RequestContext::spec`](::connectrpc::RequestContext::spec).
pub const BROWSE_SERVICE_GET_EVENT_IMPACT_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/ravelact.browse.v1.BrowseService/GetEventImpact",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the server-side `GetNode` RPC.
///
/// The dispatcher surfaces this on
/// [`RequestContext::spec`](::connectrpc::RequestContext::spec).
pub const BROWSE_SERVICE_GET_NODE_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/ravelact.browse.v1.BrowseService/GetNode",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the server-side `GetImpact` RPC.
///
/// The dispatcher surfaces this on
/// [`RequestContext::spec`](::connectrpc::RequestContext::spec).
pub const BROWSE_SERVICE_GET_IMPACT_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/ravelact.browse.v1.BrowseService/GetImpact",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the server-side `Trace` RPC.
///
/// The dispatcher surfaces this on
/// [`RequestContext::spec`](::connectrpc::RequestContext::spec).
pub const BROWSE_SERVICE_TRACE_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/ravelact.browse.v1.BrowseService/Trace",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// All RPCs are unary. The single consumer is the React SPA bundled into
/// the ravelact binary via rust-embed; the server is bound to 127.0.0.1
/// only, so there is no remote / multi-tenant story.
///
/// # Implementing handlers
///
/// Handlers receive requests as `OwnedFooView` (an alias for
/// `OwnedView<FooView<'static>>`), which gives zero-copy borrowed access
/// to fields (e.g. `request.name` is a `&str` into the decoded buffer).
/// The view can be held across `.await` points. When two RPC types in
/// the same package would alias to the same `Owned<…>View` name (e.g.
/// a local message plus an imported one with the same short name), the
/// alias is suppressed for both and the request type is spelled as
/// `OwnedView<…View<'static>>` directly in the trait signature.
///
/// Implement methods with plain `async fn`; the returned future satisfies
/// the `Send` bound automatically. See the
/// [buffa user guide](https://github.com/anthropics/buffa/blob/main/docs/guide.md#ownedview-in-async-trait-implementations)
/// for zero-copy access patterns and when `to_owned_message()` is needed.
///
/// The `impl Encodable<Out>` return bound accepts the owned `Out`, the
/// generated `OutView<'_>` / `OwnedOutView`,
/// [`MaybeBorrowed`](::connectrpc::MaybeBorrowed), or
/// [`PreEncoded`](::connectrpc::PreEncoded) for handlers that encode a
/// non-`'static` view internally and pass the bytes across the handler
/// boundary. View bodies are not emitted for output types mapped via
/// `extern_path` (the impl would be an orphan); return owned for
/// WKT/extern outputs.
///
/// Server-streaming and bidi-streaming methods return
/// `ServiceStream<impl Encodable<Out> + Send + use<Self>>`. The
/// `use<Self>` precise-capturing clause excludes `&self`'s lifetime
/// (unary methods use `use<'a, Self>` and may borrow), so stream items
/// must be `'static`. To stream view-encoded data, encode each item
/// inside the stream body and yield
/// [`PreEncoded`](::connectrpc::PreEncoded) — see its `# Streaming
/// example` doc.
#[allow(clippy::type_complexity)]
pub trait BrowseService: Send + Sync + 'static {
    /// Cytoscape graph for the SPA's main canvas.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    fn get_graph<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: OwnedGetGraphRequestView,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::cli::render::browse::proto::ravelact::browse::v1::GetGraphResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// GitHub provenance of the local repository (host/owner/repo/ref). Returns
    /// NotFound when the browse root is not a git repository, lacks an `origin`
    /// remote, points at a non-GitHub-like host, or has neither a branch nor
    /// a HEAD SHA.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    fn get_repo<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: OwnedGetRepoRequestView,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::cli::render::browse::proto::ravelact::browse::v1::GetRepoResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Per-event trigger summary across the workflow estate.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    fn list_triggers<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: OwnedListTriggersRequestView,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::cli::render::browse::proto::ravelact::browse::v1::ListTriggersResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Multi-token AND, case-insensitive substring search over the graph nodes.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    fn search<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: OwnedSearchRequestView,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::cli::render::browse::proto::ravelact::browse::v1::SearchResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Nodes reachable from workflows that list `event` as an entry trigger.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    fn get_event_impact<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: OwnedGetEventImpactRequestView,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::cli::render::browse::proto::ravelact::browse::v1::GetEventImpactResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Detail of a single node (workflow / local-action / external-action).
    /// Returns NotFound for unknown id or unsupported kind (external-workflow /
    /// docker are not first-class IR collections).
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    fn get_node<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: OwnedGetNodeRequestView,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::cli::render::browse::proto::ravelact::browse::v1::GetNodeResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Backward reachability for a workflow (everything it transitively pulls
    /// in).
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    fn get_impact<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: OwnedGetImpactRequestView,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::cli::render::browse::proto::ravelact::browse::v1::GetImpactResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Trace tree rooted at a workflow's first entry trigger. Returns NotFound
    /// when the workflow has no entry trigger (reusable-only workflows).
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    fn trace<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: OwnedTraceRequestView,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::cli::render::browse::proto::ravelact::browse::v1::TraceResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
}
/// Extension trait for registering a service implementation with a Router.
///
/// This trait is automatically implemented for all types that implement the service trait.
///
/// # Example
///
/// ```rust,ignore
/// use std::sync::Arc;
///
/// let service = Arc::new(MyServiceImpl);
/// let router = service.register(Router::new());
/// ```
pub trait BrowseServiceExt: BrowseService {
    /// Register this service implementation with a Router.
    ///
    /// Takes ownership of the `Arc<Self>` and returns a new Router with
    /// this service's methods registered.
    fn register(
        self: ::std::sync::Arc<Self>,
        router: ::connectrpc::Router,
    ) -> ::connectrpc::Router;
}
impl<S: BrowseService> BrowseServiceExt for S {
    fn register(
        self: ::std::sync::Arc<Self>,
        router: ::connectrpc::Router,
    ) -> ::connectrpc::Router {
        router
            .route_view(
                BROWSE_SERVICE_SERVICE_NAME,
                "GetGraph",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |ctx, req, format| {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            svc.get_graph(ctx, req)
                                .await?
                                .encode::<
                                    crate::cli::render::browse::proto::ravelact::browse::v1::GetGraphResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(BROWSE_SERVICE_GET_GRAPH_SPEC)
            .route_view(
                BROWSE_SERVICE_SERVICE_NAME,
                "GetRepo",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |ctx, req, format| {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            svc.get_repo(ctx, req)
                                .await?
                                .encode::<
                                    crate::cli::render::browse::proto::ravelact::browse::v1::GetRepoResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(BROWSE_SERVICE_GET_REPO_SPEC)
            .route_view(
                BROWSE_SERVICE_SERVICE_NAME,
                "ListTriggers",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |ctx, req, format| {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            svc.list_triggers(ctx, req)
                                .await?
                                .encode::<
                                    crate::cli::render::browse::proto::ravelact::browse::v1::ListTriggersResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(BROWSE_SERVICE_LIST_TRIGGERS_SPEC)
            .route_view(
                BROWSE_SERVICE_SERVICE_NAME,
                "Search",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |ctx, req, format| {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            svc.search(ctx, req)
                                .await?
                                .encode::<
                                    crate::cli::render::browse::proto::ravelact::browse::v1::SearchResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(BROWSE_SERVICE_SEARCH_SPEC)
            .route_view(
                BROWSE_SERVICE_SERVICE_NAME,
                "GetEventImpact",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |ctx, req, format| {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            svc.get_event_impact(ctx, req)
                                .await?
                                .encode::<
                                    crate::cli::render::browse::proto::ravelact::browse::v1::GetEventImpactResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(BROWSE_SERVICE_GET_EVENT_IMPACT_SPEC)
            .route_view(
                BROWSE_SERVICE_SERVICE_NAME,
                "GetNode",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |ctx, req, format| {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            svc.get_node(ctx, req)
                                .await?
                                .encode::<
                                    crate::cli::render::browse::proto::ravelact::browse::v1::GetNodeResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(BROWSE_SERVICE_GET_NODE_SPEC)
            .route_view(
                BROWSE_SERVICE_SERVICE_NAME,
                "GetImpact",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |ctx, req, format| {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            svc.get_impact(ctx, req)
                                .await?
                                .encode::<
                                    crate::cli::render::browse::proto::ravelact::browse::v1::GetImpactResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(BROWSE_SERVICE_GET_IMPACT_SPEC)
            .route_view(
                BROWSE_SERVICE_SERVICE_NAME,
                "Trace",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |ctx, req, format| {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            svc.trace(ctx, req)
                                .await?
                                .encode::<
                                    crate::cli::render::browse::proto::ravelact::browse::v1::TraceResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(BROWSE_SERVICE_TRACE_SPEC)
    }
}
/// Monomorphic dispatcher for `BrowseService`.
///
/// Unlike `.register(Router)` which type-erases each method into an `Arc<dyn ErasedHandler>` stored in a `HashMap`, this struct dispatches via a compile-time `match` on method name: no vtable, no hash lookup.
///
/// # Example
///
/// ```rust,ignore
/// use connectrpc::ConnectRpcService;
///
/// let server = BrowseServiceServer::new(MyImpl);
/// let service = ConnectRpcService::new(server);
/// // hand `service` to axum/hyper as a fallback_service
/// ```
pub struct BrowseServiceServer<T> {
    inner: ::std::sync::Arc<T>,
}
impl<T: BrowseService> BrowseServiceServer<T> {
    /// Wrap a service implementation in a monomorphic dispatcher.
    pub fn new(service: T) -> Self {
        Self {
            inner: ::std::sync::Arc::new(service),
        }
    }
    /// Wrap an already-`Arc`'d service implementation.
    pub fn from_arc(inner: ::std::sync::Arc<T>) -> Self {
        Self { inner }
    }
}
impl<T> Clone for BrowseServiceServer<T> {
    fn clone(&self) -> Self {
        Self {
            inner: ::std::sync::Arc::clone(&self.inner),
        }
    }
}
impl<T: BrowseService> ::connectrpc::Dispatcher for BrowseServiceServer<T> {
    #[inline]
    fn lookup(
        &self,
        path: &str,
    ) -> Option<::connectrpc::dispatcher::codegen::MethodDescriptor> {
        let method = path.strip_prefix("ravelact.browse.v1.BrowseService/")?;
        match method {
            "GetGraph" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(BROWSE_SERVICE_GET_GRAPH_SPEC),
                )
            }
            "GetRepo" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(BROWSE_SERVICE_GET_REPO_SPEC),
                )
            }
            "ListTriggers" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(BROWSE_SERVICE_LIST_TRIGGERS_SPEC),
                )
            }
            "Search" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(BROWSE_SERVICE_SEARCH_SPEC),
                )
            }
            "GetEventImpact" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(BROWSE_SERVICE_GET_EVENT_IMPACT_SPEC),
                )
            }
            "GetNode" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(BROWSE_SERVICE_GET_NODE_SPEC),
                )
            }
            "GetImpact" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(BROWSE_SERVICE_GET_IMPACT_SPEC),
                )
            }
            "Trace" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(BROWSE_SERVICE_TRACE_SPEC),
                )
            }
            _ => None,
        }
    }
    fn call_unary(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::Payload,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::UnaryResult {
        let Some(method) = path.strip_prefix("ravelact.browse.v1.BrowseService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_unary(path);
        };
        let _ = (&ctx, &request, &format);
        match method {
            "GetGraph" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let req = ::connectrpc::dispatcher::codegen::decode_request_view::<
                        crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetGraphRequestView,
                    >(request.encoded()?, format)?;
                    svc.get_graph(ctx, req)
                        .await?
                        .encode::<
                            crate::cli::render::browse::proto::ravelact::browse::v1::GetGraphResponse,
                        >(format)
                })
            }
            "GetRepo" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let req = ::connectrpc::dispatcher::codegen::decode_request_view::<
                        crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetRepoRequestView,
                    >(request.encoded()?, format)?;
                    svc.get_repo(ctx, req)
                        .await?
                        .encode::<
                            crate::cli::render::browse::proto::ravelact::browse::v1::GetRepoResponse,
                        >(format)
                })
            }
            "ListTriggers" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let req = ::connectrpc::dispatcher::codegen::decode_request_view::<
                        crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::ListTriggersRequestView,
                    >(request.encoded()?, format)?;
                    svc.list_triggers(ctx, req)
                        .await?
                        .encode::<
                            crate::cli::render::browse::proto::ravelact::browse::v1::ListTriggersResponse,
                        >(format)
                })
            }
            "Search" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let req = ::connectrpc::dispatcher::codegen::decode_request_view::<
                        crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::SearchRequestView,
                    >(request.encoded()?, format)?;
                    svc.search(ctx, req)
                        .await?
                        .encode::<
                            crate::cli::render::browse::proto::ravelact::browse::v1::SearchResponse,
                        >(format)
                })
            }
            "GetEventImpact" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let req = ::connectrpc::dispatcher::codegen::decode_request_view::<
                        crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetEventImpactRequestView,
                    >(request.encoded()?, format)?;
                    svc.get_event_impact(ctx, req)
                        .await?
                        .encode::<
                            crate::cli::render::browse::proto::ravelact::browse::v1::GetEventImpactResponse,
                        >(format)
                })
            }
            "GetNode" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let req = ::connectrpc::dispatcher::codegen::decode_request_view::<
                        crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetNodeRequestView,
                    >(request.encoded()?, format)?;
                    svc.get_node(ctx, req)
                        .await?
                        .encode::<
                            crate::cli::render::browse::proto::ravelact::browse::v1::GetNodeResponse,
                        >(format)
                })
            }
            "GetImpact" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let req = ::connectrpc::dispatcher::codegen::decode_request_view::<
                        crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetImpactRequestView,
                    >(request.encoded()?, format)?;
                    svc.get_impact(ctx, req)
                        .await?
                        .encode::<
                            crate::cli::render::browse::proto::ravelact::browse::v1::GetImpactResponse,
                        >(format)
                })
            }
            "Trace" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let req = ::connectrpc::dispatcher::codegen::decode_request_view::<
                        crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::TraceRequestView,
                    >(request.encoded()?, format)?;
                    svc.trace(ctx, req)
                        .await?
                        .encode::<
                            crate::cli::render::browse::proto::ravelact::browse::v1::TraceResponse,
                        >(format)
                })
            }
            _ => ::connectrpc::dispatcher::codegen::unimplemented_unary(path),
        }
    }
    fn call_server_streaming(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        request: ::buffa::bytes::Bytes,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::StreamingResult {
        let Some(method) = path.strip_prefix("ravelact.browse.v1.BrowseService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_streaming(path);
        };
        let _ = (&ctx, &request, &format);
        match method {
            _ => ::connectrpc::dispatcher::codegen::unimplemented_streaming(path),
        }
    }
    fn call_client_streaming(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        requests: ::connectrpc::dispatcher::codegen::RequestStream,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::UnaryResult {
        let Some(method) = path.strip_prefix("ravelact.browse.v1.BrowseService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_unary(path);
        };
        let _ = (&ctx, &requests, &format);
        match method {
            _ => ::connectrpc::dispatcher::codegen::unimplemented_unary(path),
        }
    }
    fn call_bidi_streaming(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        requests: ::connectrpc::dispatcher::codegen::RequestStream,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::StreamingResult {
        let Some(method) = path.strip_prefix("ravelact.browse.v1.BrowseService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_streaming(path);
        };
        let _ = (&ctx, &requests, &format);
        match method {
            _ => ::connectrpc::dispatcher::codegen::unimplemented_streaming(path),
        }
    }
}
/// Client for this service.
///
/// Generic over `T: ClientTransport`. For **gRPC** (HTTP/2), use
/// `Http2Connection` — it has honest `poll_ready` and composes with
/// `tower::balance` for multi-connection load balancing. For **Connect
/// over HTTP/1.1** (or unknown protocol), use `HttpClient`.
///
/// # Example (gRPC / HTTP/2)
///
/// ```rust,ignore
/// use connectrpc::client::{Http2Connection, ClientConfig};
/// use connectrpc::Protocol;
///
/// let uri: http::Uri = "http://localhost:8080".parse()?;
/// let conn = Http2Connection::connect_plaintext(uri.clone()).await?.shared(1024);
/// let config = ClientConfig::new(uri).with_protocol(Protocol::Grpc);
///
/// let client = BrowseServiceClient::new(conn, config);
/// let response = client.get_graph(request).await?;
/// ```
///
/// # Example (Connect / HTTP/1.1 or ALPN)
///
/// ```rust,ignore
/// use connectrpc::client::{HttpClient, ClientConfig};
///
/// let http = HttpClient::plaintext();  // cleartext http:// only
/// let config = ClientConfig::new("http://localhost:8080".parse()?);
///
/// let client = BrowseServiceClient::new(http, config);
/// let response = client.get_graph(request).await?;
/// ```
///
/// # Working with the response
///
/// Unary calls return [`UnaryResponse<OwnedView<FooView>>`](::connectrpc::client::UnaryResponse).
/// The `OwnedView` derefs to the view, so field access is zero-copy:
///
/// ```rust,ignore
/// let resp = client.get_graph(request).await?.into_view();
/// let name: &str = resp.name;  // borrow into the response buffer
/// ```
///
/// If you need the owned struct (e.g. to store or pass by value), use
/// [`into_owned()`](::connectrpc::client::UnaryResponse::into_owned):
///
/// ```rust,ignore
/// let owned = client.get_graph(request).await?.into_owned();
/// ```
#[derive(Clone)]
pub struct BrowseServiceClient<T> {
    transport: T,
    config: ::connectrpc::client::ClientConfig,
}
impl<T> BrowseServiceClient<T>
where
    T: ::connectrpc::client::ClientTransport,
    <T::ResponseBody as ::http_body::Body>::Error: ::std::fmt::Display,
{
    /// Create a new client with the given transport and configuration.
    pub fn new(transport: T, config: ::connectrpc::client::ClientConfig) -> Self {
        Self { transport, config }
    }
    /// Get the client configuration.
    pub fn config(&self) -> &::connectrpc::client::ClientConfig {
        &self.config
    }
    /// Get a mutable reference to the client configuration.
    pub fn config_mut(&mut self) -> &mut ::connectrpc::client::ClientConfig {
        &mut self.config
    }
    /// Call the GetGraph RPC. Sends a request to /ravelact.browse.v1.BrowseService/GetGraph.
    pub async fn get_graph(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::GetGraphRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetGraphResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.get_graph_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the GetGraph RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn get_graph_with_options(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::GetGraphRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetGraphResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                BROWSE_SERVICE_SERVICE_NAME,
                "GetGraph",
                request,
                options,
            )
            .await
    }
    /// Call the GetRepo RPC. Sends a request to /ravelact.browse.v1.BrowseService/GetRepo.
    pub async fn get_repo(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::GetRepoRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetRepoResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.get_repo_with_options(request, ::connectrpc::client::CallOptions::default())
            .await
    }
    /// Call the GetRepo RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn get_repo_with_options(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::GetRepoRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetRepoResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                BROWSE_SERVICE_SERVICE_NAME,
                "GetRepo",
                request,
                options,
            )
            .await
    }
    /// Call the ListTriggers RPC. Sends a request to /ravelact.browse.v1.BrowseService/ListTriggers.
    pub async fn list_triggers(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::ListTriggersRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::ListTriggersResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.list_triggers_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the ListTriggers RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn list_triggers_with_options(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::ListTriggersRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::ListTriggersResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                BROWSE_SERVICE_SERVICE_NAME,
                "ListTriggers",
                request,
                options,
            )
            .await
    }
    /// Call the Search RPC. Sends a request to /ravelact.browse.v1.BrowseService/Search.
    pub async fn search(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::SearchRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::SearchResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.search_with_options(request, ::connectrpc::client::CallOptions::default())
            .await
    }
    /// Call the Search RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn search_with_options(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::SearchRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::SearchResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                BROWSE_SERVICE_SERVICE_NAME,
                "Search",
                request,
                options,
            )
            .await
    }
    /// Call the GetEventImpact RPC. Sends a request to /ravelact.browse.v1.BrowseService/GetEventImpact.
    pub async fn get_event_impact(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::GetEventImpactRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetEventImpactResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.get_event_impact_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the GetEventImpact RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn get_event_impact_with_options(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::GetEventImpactRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetEventImpactResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                BROWSE_SERVICE_SERVICE_NAME,
                "GetEventImpact",
                request,
                options,
            )
            .await
    }
    /// Call the GetNode RPC. Sends a request to /ravelact.browse.v1.BrowseService/GetNode.
    pub async fn get_node(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::GetNodeRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetNodeResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.get_node_with_options(request, ::connectrpc::client::CallOptions::default())
            .await
    }
    /// Call the GetNode RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn get_node_with_options(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::GetNodeRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetNodeResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                BROWSE_SERVICE_SERVICE_NAME,
                "GetNode",
                request,
                options,
            )
            .await
    }
    /// Call the GetImpact RPC. Sends a request to /ravelact.browse.v1.BrowseService/GetImpact.
    pub async fn get_impact(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::GetImpactRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetImpactResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.get_impact_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the GetImpact RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn get_impact_with_options(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::GetImpactRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::GetImpactResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                BROWSE_SERVICE_SERVICE_NAME,
                "GetImpact",
                request,
                options,
            )
            .await
    }
    /// Call the Trace RPC. Sends a request to /ravelact.browse.v1.BrowseService/Trace.
    pub async fn trace(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::TraceRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::TraceResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.trace_with_options(request, ::connectrpc::client::CallOptions::default())
            .await
    }
    /// Call the Trace RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn trace_with_options(
        &self,
        request: crate::cli::render::browse::proto::ravelact::browse::v1::TraceRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::cli::render::browse::proto::ravelact::browse::v1::__buffa::view::TraceResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                BROWSE_SERVICE_SERVICE_NAME,
                "Trace",
                request,
                options,
            )
            .await
    }
}
