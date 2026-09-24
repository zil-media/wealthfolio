//! Activity edit/delete tools (MCP-only).
//!
//! `update_activity` patches an existing activity (unspecified fields keep
//! their stored values) and `delete_activity` removes one. Both are
//! destructive, so each call must carry `confirm: true`. Writes go through the
//! same core `ActivityServiceTrait` methods the activity form uses, which emit
//! the domain events that trigger portfolio recalculation.

use std::sync::Arc;

use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use wealthfolio_core::activities::{Activity, ActivityUpdate, AssetResolutionInput};

use crate::env::AgentEnvironment;
use crate::scope::AgentScope;
use crate::tool::{AgentTool, AgentToolAccess, AgentToolError, AgentToolResult};
use crate::tools::commit_activity::CommittedActivity;

/// Refuse a destructive call unless `confirm` is literally `true`. Checked on
/// the raw args before anything else so no service is touched without it.
pub(crate) fn require_confirm(args: &serde_json::Value) -> Result<(), AgentToolError> {
    if args.get("confirm") == Some(&serde_json::Value::Bool(true)) {
        Ok(())
    } else {
        Err(AgentToolError::InvalidInput(
            "This tool mutates data: pass \"confirm\": true once the user has approved the change"
                .to_string(),
        ))
    }
}

/// Audit-safe args for the manage tools: every listed id key is replaced with
/// `[redacted]`, `confirm` is kept as-is, and (for `update_activity`) only the
/// NAMES of the patched fields survive — never their values. Built as an
/// allowlist so unknown top-level keys are dropped.
pub(crate) fn redact_manage_args(args: &serde_json::Value, id_keys: &[&str]) -> serde_json::Value {
    let mut redacted = serde_json::Map::new();
    if let Some(obj) = args.as_object() {
        for key in id_keys {
            if obj.contains_key(*key) {
                redacted.insert((*key).to_string(), serde_json::json!("[redacted]"));
            }
        }
        if let Some(confirm) = obj.get("confirm").and_then(|c| c.as_bool()) {
            redacted.insert("confirm".to_string(), serde_json::json!(confirm));
        }
        if let Some(patch) = obj.get("patch").and_then(|p| p.as_object()) {
            let mut fields: Vec<&String> = patch.keys().collect();
            fields.sort();
            redacted.insert("patch".to_string(), serde_json::json!(fields));
        }
    }
    serde_json::Value::Object(redacted)
}

fn activity_summary(activity: Activity) -> CommittedActivity {
    CommittedActivity {
        id: activity.id,
        account_id: activity.account_id,
        asset_id: activity.asset_id,
        activity_type: activity.activity_type,
        activity_date: activity.activity_date.to_rfc3339(),
        currency: activity.currency,
    }
}

pub(crate) fn required_id(value: &str, field: &str) -> Result<String, AgentToolError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AgentToolError::InvalidInput(format!("{field} is required")));
    }
    Ok(trimmed.to_string())
}

/// Editable activity fields. Omitted fields keep their stored value.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityPatch {
    pub quantity: Option<f64>,
    pub unit_price: Option<f64>,
    pub amount: Option<f64>,
    pub fee: Option<f64>,
    pub tax: Option<f64>,
    pub activity_date: Option<String>,
    pub currency: Option<String>,
    /// Empty string clears the notes.
    pub notes: Option<String>,
    pub activity_type: Option<String>,
    /// Empty string clears the subtype.
    pub subtype: Option<String>,
    pub account_id: Option<String>,
    /// Must reference an existing asset; never creates one.
    pub asset_id: Option<String>,
}

impl ActivityPatch {
    fn is_empty(&self) -> bool {
        self.quantity.is_none()
            && self.unit_price.is_none()
            && self.amount.is_none()
            && self.fee.is_none()
            && self.tax.is_none()
            && self.activity_date.is_none()
            && self.currency.is_none()
            && self.notes.is_none()
            && self.activity_type.is_none()
            && self.subtype.is_none()
            && self.account_id.is_none()
            && self.asset_id.is_none()
    }
}

/// Build the core update from the stored activity plus the patch. Fields the
/// patch omits are carried over from `existing` (or left as `None` where the
/// core update treats `None` as "preserve").
fn build_activity_update(
    existing: &Activity,
    patch: &ActivityPatch,
) -> Result<ActivityUpdate, AgentToolError> {
    let decimal_patch =
        |value: Option<f64>, field: &str| -> Result<Option<Option<Decimal>>, AgentToolError> {
            match value {
                None => Ok(None),
                Some(v) => Decimal::from_f64(v).map(|d| Some(Some(d))).ok_or_else(|| {
                    AgentToolError::InvalidInput(format!("Invalid numeric value for {field}: {v}"))
                }),
            }
        };
    let text_or = |value: &Option<String>, fallback: &str| -> String {
        value
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or(fallback)
            .to_string()
    };

    let asset = match patch.asset_id.as_deref() {
        None => None, // core preserves the stored asset
        Some(id) => Some(AssetResolutionInput {
            id: Some(required_id(id, "patch.assetId")?),
            ..Default::default()
        }),
    };
    let notes = match patch.notes.as_deref() {
        None => existing.notes.clone(),
        Some(n) if n.trim().is_empty() => None,
        Some(n) => Some(n.to_string()),
    };

    Ok(ActivityUpdate {
        id: existing.id.clone(),
        account_id: text_or(&patch.account_id, &existing.account_id),
        asset,
        // The effective type (override included) is what the activity form
        // edits; sending the base type would clear a user override.
        activity_type: text_or(&patch.activity_type, existing.effective_type()),
        subtype: patch.subtype.clone(),
        activity_date: text_or(&patch.activity_date, &existing.activity_date.to_rfc3339()),
        quantity: decimal_patch(patch.quantity, "quantity")?,
        unit_price: decimal_patch(patch.unit_price, "unitPrice")?,
        currency: text_or(&patch.currency, &existing.currency),
        fee: decimal_patch(patch.fee, "fee")?,
        tax: decimal_patch(patch.tax, "tax")?,
        amount: decimal_patch(patch.amount, "amount")?,
        status: None,
        needs_review: None,
        notes,
        fx_rate: None,
        metadata: None,
    })
}

/// Output for `update_activity`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateActivityOutput {
    pub updated: CommittedActivity,
}

/// Output for `delete_activity`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteActivityOutput {
    pub deleted: CommittedActivity,
}

/// Patch an existing activity.
pub struct UpdateActivity;

#[async_trait::async_trait]
impl AgentTool for UpdateActivity {
    fn name(&self) -> &'static str {
        "update_activity"
    }

    fn description(&self) -> &'static str {
        "Edit an existing activity by id. Only the fields in `patch` change; \
         everything else keeps its stored value. `patch.assetId` must be an \
         existing asset (it is never created). This MUTATES data — pass \
         confirm: true only after the user approved the change."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Activity id." },
                "patch": {
                    "type": "object",
                    "description": "Fields to change. Omitted fields are unchanged.",
                    "properties": {
                        "quantity": { "type": "number" },
                        "unitPrice": { "type": "number" },
                        "amount": { "type": "number" },
                        "fee": { "type": "number" },
                        "tax": { "type": "number" },
                        "activityDate": { "type": "string", "description": "ISO 8601 date." },
                        "currency": { "type": "string" },
                        "notes": { "type": "string", "description": "Empty string clears." },
                        "activityType": { "type": "string" },
                        "subtype": { "type": "string", "description": "Empty string clears." },
                        "accountId": { "type": "string" },
                        "assetId": { "type": "string", "description": "Existing asset id." }
                    },
                    "additionalProperties": false
                },
                "confirm": {
                    "type": "boolean",
                    "const": true,
                    "description": "Must be true to apply the change."
                }
            },
            "required": ["id", "patch", "confirm"]
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
            patch: ActivityPatch,
        }
        let args: Args = serde_json::from_value(args)?;
        let id = required_id(&args.id, "id")?;
        if args.patch.is_empty() {
            return Err(AgentToolError::InvalidInput(
                "patch must set at least one field".to_string(),
            ));
        }

        let activity_service = env.activity_service();
        let existing = activity_service
            .get_activity(&id)
            .map_err(|e| AgentToolError::ExecutionFailed(e.to_string()))?;
        if let Some(asset_id) = args.patch.asset_id.as_deref() {
            let asset_id = required_id(asset_id, "patch.assetId")?;
            // Error text lands in the audit log: name the field, never the id.
            env.asset_service()
                .get_asset_by_id(&asset_id)
                .map_err(|_| {
                    AgentToolError::InvalidInput(
                        "patch.assetId does not reference an existing asset".to_string(),
                    )
                })?;
        }

        let update = build_activity_update(&existing, &args.patch)?;
        let updated = activity_service
            .update_activity(update)
            .await
            .map_err(|e| AgentToolError::ExecutionFailed(e.to_string()))?;
        env.health_service().clear_cache().await;

        Ok(AgentToolResult {
            content: serde_json::to_value(UpdateActivityOutput {
                updated: activity_summary(updated),
            })?,
        })
    }
}

/// Delete an activity.
pub struct DeleteActivity;

#[async_trait::async_trait]
impl AgentTool for DeleteActivity {
    fn name(&self) -> &'static str {
        "delete_activity"
    }

    fn description(&self) -> &'static str {
        "Permanently delete an activity by id (a linked transfer's other leg is \
         deleted with it). This MUTATES data — pass confirm: true only after the \
         user approved the deletion."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Activity id." },
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

        let deleted = env
            .activity_service()
            .delete_activity(id)
            .await
            .map_err(|e| AgentToolError::ExecutionFailed(e.to_string()))?;
        env.health_service().clear_cache().await;

        Ok(AgentToolResult {
            content: serde_json::to_value(DeleteActivityOutput {
                deleted: activity_summary(deleted),
            })?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use wealthfolio_core::activities::ActivityStatus;

    fn stored_buy() -> Activity {
        Activity {
            id: "act-1".to_string(),
            account_id: "acct-1".to_string(),
            asset_id: Some("asset-1".to_string()),
            activity_type: "BUY".to_string(),
            activity_type_override: None,
            source_type: None,
            subtype: Some("DRIP".to_string()),
            status: ActivityStatus::Posted,
            activity_date: Utc.with_ymd_and_hms(2024, 1, 15, 0, 0, 0).unwrap(),
            settlement_date: None,
            quantity: Some(Decimal::from(10)),
            unit_price: Some(Decimal::from(150)),
            amount: None,
            fee: Some(Decimal::from(1)),
            tax: None,
            currency: "USD".to_string(),
            fx_rate: None,
            notes: Some("keep me".to_string()),
            metadata: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
            import_run_id: None,
            is_user_modified: false,
            needs_review: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn patch_changes_only_the_given_fields() {
        let patch: ActivityPatch =
            serde_json::from_value(serde_json::json!({ "fee": 2.5, "currency": "EUR" })).unwrap();
        let update = build_activity_update(&stored_buy(), &patch).unwrap();

        assert_eq!(update.id, "act-1");
        assert_eq!(update.fee, Some(Some(Decimal::from_f64(2.5).unwrap())));
        assert_eq!(update.currency, "EUR");
        // Unpatched fields are carried over or left as core "preserve" (None).
        assert_eq!(update.account_id, "acct-1");
        assert_eq!(update.activity_type, "BUY");
        assert_eq!(update.activity_date, "2024-01-15T00:00:00+00:00");
        assert_eq!(update.notes.as_deref(), Some("keep me"));
        assert!(update.asset.is_none(), "omitted asset is preserved by core");
        assert!(
            update.subtype.is_none(),
            "omitted subtype is preserved by core"
        );
        assert!(update.quantity.is_none());
        assert!(update.unit_price.is_none());
        assert!(update.status.is_none());
        assert!(update.needs_review.is_none());
    }

    #[test]
    fn patch_asset_id_is_sent_as_an_id_only_reference() {
        let patch = ActivityPatch {
            asset_id: Some(" asset-2 ".to_string()),
            ..Default::default()
        };
        let update = build_activity_update(&stored_buy(), &patch).unwrap();
        let asset = update.asset.expect("asset patch");
        assert_eq!(asset.id.as_deref(), Some("asset-2"));
        // No symbol: core resolves by id and never mints a new asset.
        assert!(asset.symbol.is_none());
    }

    #[test]
    fn empty_notes_and_subtype_clear() {
        let patch = ActivityPatch {
            notes: Some(String::new()),
            subtype: Some(String::new()),
            ..Default::default()
        };
        let update = build_activity_update(&stored_buy(), &patch).unwrap();
        assert!(update.notes.is_none());
        assert_eq!(update.subtype.as_deref(), Some(""));
    }

    #[test]
    fn unpatched_type_keeps_user_override() {
        let mut existing = stored_buy();
        existing.activity_type_override = Some("SELL".to_string());
        let update = build_activity_update(&existing, &ActivityPatch::default()).unwrap();
        assert_eq!(update.activity_type, "SELL");
    }

    #[test]
    fn non_finite_patch_value_is_rejected() {
        let patch = ActivityPatch {
            quantity: Some(f64::NAN),
            ..Default::default()
        };
        assert!(matches!(
            build_activity_update(&stored_buy(), &patch),
            Err(AgentToolError::InvalidInput(_))
        ));
    }

    #[test]
    fn unknown_patch_field_is_rejected() {
        let result: Result<ActivityPatch, _> =
            serde_json::from_value(serde_json::json!({ "symbol": "AAPL" }));
        assert!(result.is_err());
    }

    #[test]
    fn confirm_must_be_literally_true() {
        assert!(require_confirm(&serde_json::json!({ "confirm": true })).is_ok());
        for args in [
            serde_json::json!({}),
            serde_json::json!({ "confirm": false }),
            serde_json::json!({ "confirm": "true" }),
            serde_json::json!({ "confirm": 1 }),
        ] {
            assert!(matches!(
                require_confirm(&args),
                Err(AgentToolError::InvalidInput(_))
            ));
        }
    }

    #[test]
    fn audit_redaction_keeps_field_names_only() {
        let args = serde_json::json!({
            "id": "act-1",
            "patch": { "notes": "secret", "amount": 99.0 },
            "confirm": true,
            "extra": "SSN 123-45-6789",
        });
        let redacted = UpdateActivity.sanitize_args_for_audit(&args);
        assert_eq!(
            redacted,
            serde_json::json!({
                "id": "[redacted]",
                "patch": ["amount", "notes"],
                "confirm": true,
            })
        );
        let redacted = DeleteActivity
            .sanitize_args_for_audit(&serde_json::json!({ "id": "act-1", "confirm": true }));
        assert_eq!(
            redacted,
            serde_json::json!({ "id": "[redacted]", "confirm": true })
        );
    }
}
