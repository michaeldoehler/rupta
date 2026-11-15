//! The MCP server handler: server information, tools, resources and
//! prompts.

use axum::Router;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, GetPromptRequestParams, GetPromptResponse,
    Implementation, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use rupta_api::ApiState;
use rupta_api::extract::Auth;

use crate::rest::Rest;

/// Instructions sent to clients at initialization.
const INSTRUCTIONS: &str = "rupta is an error tracking and application monitoring service. \
Start with list_organizations to learn the organization slugs, then list_projects. \
search_issues finds error groups (issues) with rupta's query language (read the resource rupta://docs/search for the syntax); \
get_issue gives the details of one issue and get_event a readable stack trace with breadcrumbs, request and context. \
Traces, logs, metrics, releases, cron monitors and alerts have their own tools. \
Tools that change something (update_issue, add_comment, create_ticket, link_ticket) need a token with the issue:write \
scope and at least the member role; ask the user before changing issues they did not mention. \
Ids are strings; issues also have short ids such as WEB-12 (pass org with them).";

/// The MCP server; one instance serves every request.
#[derive(Clone)]
pub struct RuptaMcp {
    api: Router,
    tools: std::sync::Arc<Vec<Tool>>,
}

impl RuptaMcp {
    pub fn new(state: ApiState) -> RuptaMcp {
        let api = rupta_api::router(state);
        RuptaMcp { api, tools: std::sync::Arc::new(crate::tools::definitions()) }
    }

    /// The REST client acting as the caller of this request.
    pub(crate) fn rest(&self, ctx: &RequestContext<RoleServer>) -> Result<Rest, ErrorData> {
        let parts = ctx
            .extensions
            .get::<http::request::Parts>()
            .ok_or_else(|| ErrorData::internal_error("request without HTTP context", None))?;
        let auth = parts
            .extensions
            .get::<Auth>()
            .cloned()
            .ok_or_else(|| ErrorData::internal_error("request without authentication", None))?;
        Ok(Rest::new(self.api.clone(), auth, parts))
    }
}

impl ServerHandler for RuptaMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().enable_prompts().build())
            .with_server_info(
                Implementation::new("rupta", env!("CARGO_PKG_VERSION"))
                    .with_title("rupta")
                    .with_description("Errors, traces, logs, metrics, releases and alerts from rupta"),
            )
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.as_ref().clone()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|t| t.name == name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if self.get_tool(&request.name).is_none() {
            return Err(ErrorData::invalid_params(format!("unknown tool {}", request.name), None));
        }
        let rest = self.rest(&context)?;
        let args = request.arguments.unwrap_or_default();
        let result = match crate::tools::call(&rest, &request.name, args).await {
            Ok(out) => out.into_result(),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e.0)]),
        };
        Ok(result.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(crate::resources::list()))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(crate::resources::templates()))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let rest = self.rest(&context)?;
        let result = crate::resources::read(&rest, &request.uri).await?;
        Ok(result.into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(ListPromptsResult::with_all_items(crate::prompts::list()))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        Ok(crate::prompts::get(&request.name, request.arguments.unwrap_or_default())?.into())
    }
}
