pub mod catalog;
pub mod framing;
pub mod http;
pub mod validation;
use crate::{service, storage::Store};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, model::*, service::RequestContext};
use serde_json::{Value, json};
use std::borrow::Cow;
#[derive(Clone)]
pub struct Handler {
    pub store: Store,
}
impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().enable_prompts().build()).with_server_info(Implementation::new("fiverr-agent-mcp",env!("CARGO_PKG_VERSION"))).with_instructions("Unofficial local workspace. Drafts/imports/handoffs are not live Fiverr results. Never disclose credentials or self-approve. Account automation is gated.")
    }
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25])
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        if request.and_then(|r| r.cursor).is_some() {
            return Err(ErrorData::invalid_params("No next catalog page.", None));
        }
        Ok(ListToolsResult {
            tools: catalog::tools(),
            ..Default::default()
        })
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        catalog::tools().into_iter().find(|t| t.name == name)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        if !catalog::TOOLS.iter().any(|t| t.0 == request.name) {
            return Err(ErrorData::invalid_params(
                "Unknown implemented tool; inspect discovery.",
                None,
            ));
        }
        let args = Value::Object(request.arguments.unwrap_or_default());
        let result = match service::dispatch(&self.store, &request.name, args) {
            Ok(value) => {
                let mut result =
                    CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
                result.structured_content = Some(value);
                result
            }
            Err(e) => {
                let value = json!({"error":{"code":e.code.as_str(),"message":e.message}});
                let mut result = CallToolResult::error(vec![ContentBlock::text(value.to_string())]);
                result.structured_content = Some(value);
                result
            }
        };
        Ok(result.into())
    }
    async fn list_resources(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult {
            resources: vec![
                Resource::new("fiverr://capabilities", "capabilities"),
                Resource::new("fiverr://policy", "policy"),
                Resource::new("fiverr://account", "account-status"),
            ],
            ..Default::default()
        })
    }
    async fn list_resource_templates(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![
                ResourceTemplate::new("fiverr://records/{kind}/{id}", "workspace-record"),
                ResourceTemplate::new("fiverr://actions/{id}", "prepared-action"),
            ],
            ..Default::default()
        })
    }
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResponse, ErrorData> {
        let uri = &request.uri;
        let value=match uri.as_str(){
            "fiverr://capabilities"=>catalog::capabilities(),
            "fiverr://account"=>service::account::status(&self.store),
            "fiverr://policy"=>Ok(json!({"outbound_default":"blocked","approval":"owner_control_only","financial_execution":false,"platform_automation":"permission_and_route_assurance_required","arbitrary_execution":false})),
            value if value.starts_with("fiverr://records/")=>{
                let parts:Vec<_>=value.trim_start_matches("fiverr://records/").split('/').collect();
                if parts.len()!=2{return Err(ErrorData::resource_not_found("Unknown resource URI.",None));}
                service::local::get(&self.store,&json!({"kind":parts[0],"id":parts[1]}))
            },
            value if value.starts_with("fiverr://actions/")=>service::actions::get(&self.store,value.trim_start_matches("fiverr://actions/")),
            _=>return Err(ErrorData::resource_not_found("Unknown resource URI.",None)),
        }.map_err(|e|ErrorData::resource_not_found(e.message,None))?;
        Ok(
            ReadResourceResult::new(vec![ResourceContents::text(value.to_string(), uri.clone())])
                .into(),
        )
    }
    async fn list_prompts(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListPromptsResult, ErrorData> {
        let prompt:Prompt=serde_json::from_value(json!({"name":"fiverr-gig-optimizer","description":"Reviewed adapted third-party optimizer instructions; MIT helper code and CC-BY-4.0 documentation/data."})).map_err(|_|ErrorData::internal_error("Bundled prompt metadata invalid.",None))?;
        Ok(ListPromptsResult {
            prompts: vec![prompt],
            ..Default::default()
        })
    }
    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<GetPromptResponse, ErrorData> {
        if request.name != "fiverr-gig-optimizer" {
            return Err(ErrorData::invalid_params("Unknown bundled prompt.", None));
        }
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            Role::User,
            include_str!("../../skills/fiverr-gig-optimizer/SKILL.md"),
        )])
        .into())
    }
}
pub async fn bridge(store: Store) -> crate::core::Result<()> {
    let transport = framing::BoundedStdio::new(tokio::io::stdin(), tokio::io::stdout());
    let server = Handler { store }.serve(transport).await.map_err(|_| {
        crate::core::invalid("MCP startup failed; check protocol/version/frame metadata.")
    })?;
    server
        .waiting()
        .await
        .map_err(|_| crate::core::invalid("MCP transport terminated with an error."))?;
    Ok(())
}
