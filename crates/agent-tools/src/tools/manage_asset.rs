//! Asset cleanup tools (MCP-only).
//!
//! `delete_asset` removes an asset that no activity references, and
//! `merge_assets` folds a duplicate asset into another one (activities are
//! reassigned, then the source is deleted). Both require `confirm: true`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::env::AgentEnvironment;
use crate::scope::AgentScope;
use crate::tool::{AgentTool, AgentToolAccess, AgentToolError, AgentToolResult};
use crate::tools::manage_activity::{redact_manage_args, require_confirm, required_id};

/// Output for `delete_asset`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAssetOutput {
    pub deleted_asset_id: String,
}

/// Output for `merge_assets`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeAssetsOutput {
    pub source_id: String,
    pub target_id: String,
    pub activities_migrated: u32,
}

/// Delete an asset with no activities.
pub struct DeleteAsset;

#[async_trait::async_trait]
impl AgentTool for DeleteAsset {
    fn name(&self) -> &'static str {
        "delete_asset"
    }

    fn description(&self) -> &'static str {
        "Delete an asset (security) that has no activities, together with its \
         quotes and classifications. Fails if any activity still references it — \
         use merge_assets to fold a duplicate into another asset instead. This \
         MUTATES data — pass confirm: true only after the user approved it."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Asset id." },
                "confirm": {
                    "type": "boolean",
                    "const": true,
                    "description": "Must be true to delete."
                }
            },
            "required": ["id", "confirm"]
        })
    }

    fn required_scopes(&self) -> &'static [AgentScope] {
        &[AgentScope::ActivitiesDraft, AgentScope::ActivitiesWrite]
    }

    fn access_level(&self) -> AgentToolAccess {
        AgentToolAccess::Write
    }

    fn sanitize_args_for_audit(&self, args: &serde_json::Value) -> serde_json::Value {
        redact_manage_args(args, &["id"])
    }

    async fn call(
        &self,
        env: Arc<dyn AgentEnvironment>,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        require_confirm(&args)?;
        #[derive(Deserialize)]
        struct Args {
            id: String,
        }
        let args: Args = serde_json::from_value(args)?;
        let id = required_id(&args.id, "id")?;

        // Error text lands in the audit log: never echo the id back.
        let asset_service = env.asset_service();
        asset_service.get_asset_by_id(&id).map_err(|_| {
            AgentToolError::InvalidInput("id does not reference an existing asset".to_string())
        })?;

        let activity_count = env
            .activity_service()
            .get_activities()
            .map_err(|e| AgentToolError::ExecutionFailed(e.to_string()))?
            .iter()
            .filter(|activity| activity.asset_id.as_deref() == Some(id.as_str()))
            .count();
        if activity_count > 0 {
            return Err(AgentToolError::InvalidInput(format!(
                "Asset still has {activity_count} activities; delete them or use \
                 merge_assets to move them to another asset first"
            )));
        }

        // The repository re-checks the constraint (including archived
        // accounts) inside the delete transaction.
        asset_service
            .delete_asset(&id)
            .await
            .map_err(|e| AgentToolError::ExecutionFailed(e.to_string()))?;
        env.health_service().clear_cache().await;

        Ok(AgentToolResult {
            content: serde_json::to_value(DeleteAssetOutput {
                deleted_asset_id: id,
            })?,
        })
    }
}

/// Merge one asset into another.
pub struct MergeAssets;

#[async_trait::async_trait]
impl AgentTool for MergeAssets {
    fn name(&self) -> &'static str {
        "merge_assets"
    }

    fn description(&self) -> &'static str {
        "Merge a duplicate asset into another existing asset: every activity of \
         sourceId is reassigned to targetId, then sourceId is deleted and \
         holdings are recalculated. This MUTATES data — pass confirm: true only \
         after the user approved it."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "sourceId": { "type": "string", "description": "Asset to merge away (deleted)." },
                "targetId": { "type": "string", "description": "Existing asset that receives the activities." },
                "confirm": {
                    "type": "boolean",
                    "const": true,
                    "description": "Must be true to merge."
                }
            },
            "required": ["sourceId", "targetId", "confirm"]
        })
    }

    fn required_scopes(&self) -> &'static [AgentScope] {
        &[AgentScope::ActivitiesDraft, AgentScope::ActivitiesWrite]
    }

    fn access_level(&self) -> AgentToolAccess {
        AgentToolAccess::Write
    }

    fn sanitize_args_for_audit(&self, args: &serde_json::Value) -> serde_json::Value {
        redact_manage_args(args, &["sourceId", "targetId"])
    }

    async fn call(
        &self,
        env: Arc<dyn AgentEnvironment>,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        require_confirm(&args)?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Args {
            source_id: String,
            target_id: String,
        }
        let args: Args = serde_json::from_value(args)?;
        let source_id = required_id(&args.source_id, "sourceId")?;
        let target_id = required_id(&args.target_id, "targetId")?;
        if source_id == target_id {
            return Err(AgentToolError::InvalidInput(
                "Cannot merge an asset into itself".to_string(),
            ));
        }

        let asset_service = env.asset_service();
        for (field, id) in [("sourceId", &source_id), ("targetId", &target_id)] {
            asset_service.get_asset_by_id(id).map_err(|_| {
                AgentToolError::InvalidInput(format!(
                    "{field} does not reference an existing asset"
                ))
            })?;
        }

        let activities_migrated = env
            .activity_service()
            .merge_assets(&source_id, &target_id)
            .await
            .map_err(|e| AgentToolError::ExecutionFailed(e.to_string()))?;
        env.health_service().clear_cache().await;

        Ok(AgentToolResult {
            content: serde_json::to_value(MergeAssetsOutput {
                source_id,
                target_id,
                activities_migrated,
            })?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_redaction_hides_asset_ids() {
        let redacted = MergeAssets.sanitize_args_for_audit(&serde_json::json!({
            "sourceId": "a",
            "targetId": "b",
            "confirm": true,
            "note": "x",
        }));
        assert_eq!(
            redacted,
            serde_json::json!({
                "sourceId": "[redacted]",
                "targetId": "[redacted]",
                "confirm": true,
            })
        );
        let redacted = DeleteAsset
            .sanitize_args_for_audit(&serde_json::json!({ "id": "a", "confirm": false }));
        assert_eq!(
            redacted,
            serde_json::json!({ "id": "[redacted]", "confirm": false })
        );
    }

    #[test]
    fn schemas_require_confirm() {
        for tool in [
            &DeleteAsset as &dyn AgentTool,
            &MergeAssets as &dyn AgentTool,
        ] {
            let required = tool.input_schema()["required"].clone();
            assert!(
                required
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("confirm")),
                "{} must require confirm",
                tool.name()
            );
        }
    }
}
