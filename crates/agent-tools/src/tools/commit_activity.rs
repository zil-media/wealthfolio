//! Commit Activity tools (MCP-only).
//!
//! `commit_activity_draft` and `commit_activity_drafts` take the
//! [`ActivityDraft`] shape that `record_activity` / `record_activities`
//! produce and persist them as real activities. These are the write step the
//! in-app assistant performs through its confirmation widget (the frontend
//! calls the activity form mutation). MCP callers have no widget, so these
//! tools let a scoped token commit a previously-reviewed draft directly.
//!
//! Mapping mirrors the frontend draft → create payload (record-activity-tool-ui):
//! the draft's `assetId`/`symbol`/`assetKind` become the nested
//! [`AssetResolutionInput`]; numeric fields are converted f64 → Decimal.
//!
//! Asset resolution is strict (see [`resolve_asset_plan`]): an existing
//! `assetId` is written to as-is or the draft fails, an ambiguous symbol is an
//! error, and a new asset is only created with `createAssetIfMissing: true`.

use std::sync::Arc;

use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use wealthfolio_core::activities::{AssetResolutionInput, NewActivity};
use wealthfolio_core::assets::{parse_symbol_with_exchange_suffix, Asset, AssetKind};

use crate::env::AgentEnvironment;
use crate::scope::AgentScope;
use crate::tool::{AgentTool, AgentToolAccess, AgentToolError, AgentToolResult};
use crate::tools::record_activity::ActivityDraft;

/// Max drafts per `commit_activity_drafts` call. Mirrors the `record_activities`
/// batch cap (these drafts come from there), so a buggy or hostile client with
/// write scope can't trigger an unbounded one-by-one write.
const MAX_COMMIT_DRAFTS: usize = 100;

/// Drop the draft payload(s) from audit args — never persist amounts, account
/// ids, symbols, or notes in `mcp_audit_log`. Built as an allowlist (fresh
/// object) so an extra top-level field an agent attaches can't slip through
/// unredacted.
fn redact_drafts(args: &serde_json::Value) -> serde_json::Value {
    let mut redacted = serde_json::Map::new();
    if let Some(obj) = args.as_object() {
        if obj.contains_key("draft") {
            redacted.insert("draft".to_string(), serde_json::json!("[redacted]"));
        }
        if let Some(drafts) = obj.get("drafts").and_then(|d| d.as_array()) {
            redacted.insert(
                "drafts".to_string(),
                serde_json::json!(format!("[{} drafts]", drafts.len())),
            );
        }
    }
    serde_json::Value::Object(redacted)
}

/// A draft as submitted for commit: the [`ActivityDraft`] fields plus
/// commit-only options.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDraftInput {
    #[serde(flatten)]
    pub draft: ActivityDraft,
    /// Caller-chosen dedupe key (e.g. the broker order number). Committing a
    /// second draft with the same key fails with the existing activity id.
    pub idempotency_key: Option<String>,
    /// Allow creating a new asset when the draft names no existing one.
    #[serde(default)]
    pub create_asset_if_missing: bool,
}

/// Arguments for `commit_activity_drafts`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitActivityDraftsArgs {
    pub drafts: Vec<CommitDraftInput>,
    /// Resolve every draft's target asset without writing anything.
    #[serde(default)]
    pub dry_run: bool,
}

/// What committing a draft does to assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssetAction {
    /// Writes to the existing asset `assetId`.
    Existing,
    /// Creates a new asset from the draft's symbol.
    Create,
    /// Pure cash activity; no asset.
    None,
}

/// The asset a draft resolves to, computed before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPlan {
    pub asset_id: Option<String>,
    pub asset_action: AssetAction,
}

/// Dry-run result for one draft.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedCommit {
    pub index: usize,
    #[serde(flatten)]
    pub plan: AssetPlan,
}

/// Summary of a committed activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommittedActivity {
    pub id: String,
    pub account_id: String,
    pub asset_id: Option<String>,
    pub activity_type: String,
    pub activity_date: String,
    pub currency: String,
}

/// Output for `commit_activity_draft`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitActivityDraftOutput {
    pub created: CommittedActivity,
}

/// A row-level error in a batch commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitError {
    pub index: usize,
    pub message: String,
}

/// Output for `commit_activity_drafts` (partial success).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitActivityDraftsOutput {
    pub created: Vec<CommittedActivity>,
    pub errors: Vec<CommitError>,
}

/// Output for `commit_activity_drafts` with `dryRun: true`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitActivityDraftsDryRunOutput {
    pub dry_run: bool,
    pub planned: Vec<PlannedCommit>,
    pub errors: Vec<CommitError>,
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// True when `symbol` names `asset`: its display code, or its instrument
/// symbol once a Yahoo exchange suffix is split off (on that venue).
fn asset_names_symbol(asset: &Asset, symbol: &str) -> bool {
    if asset
        .display_code
        .as_deref()
        .is_some_and(|code| code.eq_ignore_ascii_case(symbol))
    {
        return true;
    }
    let (base, suffix_mic) = parse_symbol_with_exchange_suffix(symbol);
    asset
        .instrument_symbol
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case(base))
        && suffix_mic.is_none_or(|mic| {
            asset
                .instrument_exchange_mic
                .as_deref()
                .is_some_and(|m| m.eq_ignore_ascii_case(mic))
        })
}

fn asset_label(asset: &Asset) -> String {
    let instrument_type = asset
        .instrument_type
        .as_ref()
        .map(|t| t.as_db_str().to_string())
        .unwrap_or_else(|| format!("{:?}", asset.kind).to_uppercase());
    match asset.name.as_deref() {
        Some(name) => format!("{} ({instrument_type}, {name})", asset.id),
        None => format!("{} ({instrument_type})", asset.id),
    }
}

/// Decide which asset a draft writes to, without writing anything.
///
/// - `assetId` of an existing asset: that asset, or an error if the draft's
///   symbol names something else. Never re-resolved by symbol.
/// - Otherwise by symbol (a non-existing `assetId` is a provider-derived id
///   such as `AAPL:XNAS` from `record_activity`): exactly one existing match is
///   used; several are an error; none creates an asset only with
///   `createAssetIfMissing: true`.
pub fn resolve_asset_plan(
    assets: &[Asset],
    input: &CommitDraftInput,
) -> Result<AssetPlan, AgentToolError> {
    let draft = &input.draft;
    let symbol = non_empty(draft.symbol.as_deref());

    if let Some(asset_id) = non_empty(draft.asset_id.as_deref()) {
        if let Some(asset) = assets.iter().find(|a| a.id == asset_id) {
            if let Some(symbol) = symbol {
                if !asset_names_symbol(asset, symbol) {
                    return Err(AgentToolError::InvalidInput(format!(
                        "assetId {} does not match symbol {symbol}. Fix the symbol or the assetId.",
                        asset_label(asset)
                    )));
                }
            }
            return Ok(AssetPlan {
                asset_id: Some(asset.id.clone()),
                asset_action: AssetAction::Existing,
            });
        }
        if symbol.is_none() {
            return Err(AgentToolError::InvalidInput(format!(
                "assetId {asset_id} does not exist"
            )));
        }
    }

    let Some(symbol) = symbol else {
        return Ok(AssetPlan {
            asset_id: None,
            asset_action: AssetAction::None,
        });
    };

    let matches: Vec<&Asset> = assets
        .iter()
        .filter(|a| a.is_active && a.kind != AssetKind::Fx && asset_names_symbol(a, symbol))
        .collect();
    match matches.as_slice() {
        [asset] => Ok(AssetPlan {
            asset_id: Some(asset.id.clone()),
            asset_action: AssetAction::Existing,
        }),
        [] if input.create_asset_if_missing => Ok(AssetPlan {
            asset_id: None,
            asset_action: AssetAction::Create,
        }),
        [] => Err(AgentToolError::InvalidInput(format!(
            "No existing asset matches symbol {symbol}. Set createAssetIfMissing: true to create one."
        ))),
        many => Err(AgentToolError::InvalidInput(format!(
            "Symbol {symbol} is ambiguous: it matches {}. Pass assetId to choose one.",
            many.iter()
                .map(|a| asset_label(a))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Convert an [`ActivityDraft`] into the core [`NewActivity`] create payload.
///
/// Mirrors the frontend draft → create payload mapping: asset identity is
/// nested into [`AssetResolutionInput`] and numeric fields are converted from
/// f64 to `Decimal`. An existing asset from `plan` is passed by id alone, so
/// core cannot re-resolve it by symbol onto a different asset.
fn draft_to_new_activity(
    draft: &ActivityDraft,
    plan: &AssetPlan,
    idempotency_key: Option<&str>,
) -> Result<NewActivity, AgentToolError> {
    let account_id = draft
        .account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| AgentToolError::InvalidInput("Draft is missing an account_id".to_string()))?
        .to_string();

    let asset = match plan.asset_action {
        AssetAction::None => None,
        AssetAction::Existing => Some(AssetResolutionInput {
            id: plan.asset_id.clone(),
            ..Default::default()
        }),
        AssetAction::Create => Some(AssetResolutionInput {
            id: None,
            symbol: draft
                .symbol
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            exchange_mic: None,
            kind: draft.asset_kind.clone(),
            name: draft.asset_name.clone(),
            quote_mode: Some(draft.pricing_mode.clone()),
            quote_ccy: None,
            instrument_type: None,
            provider_id: None,
            provider_symbol: None,
        }),
    };

    let to_decimal = |value: Option<f64>, field: &str| -> Result<Option<Decimal>, AgentToolError> {
        match value {
            None => Ok(None),
            Some(v) => Decimal::from_f64(v).map(Some).ok_or_else(|| {
                AgentToolError::InvalidInput(format!("Invalid numeric value for {field}: {v}"))
            }),
        }
    };

    Ok(NewActivity {
        id: None,
        account_id,
        asset,
        activity_type: draft.activity_type.clone(),
        subtype: draft.subtype.clone(),
        activity_date: draft.activity_date.clone(),
        quantity: to_decimal(draft.quantity, "quantity")?,
        unit_price: to_decimal(draft.unit_price, "unitPrice")?,
        currency: draft.currency.clone(),
        fee: to_decimal(draft.fee, "fee")?,
        tax: to_decimal(draft.tax, "tax")?,
        amount: to_decimal(draft.amount, "amount")?,
        status: None,
        notes: draft.notes.clone(),
        fx_rate: None,
        metadata: None,
        // Committed drafts are previously reviewed (see module docs), so a
        // custom trade total is attested rather than queued for review again.
        needs_review: Some(false),
        source_system: None,
        source_record_id: None,
        source_group_id: None,
        idempotency_key: non_empty(idempotency_key).map(str::to_string),
        import_run_id: None,
    })
}

fn committed_summary(activity: wealthfolio_core::activities::Activity) -> CommittedActivity {
    CommittedActivity {
        id: activity.id,
        account_id: activity.account_id,
        asset_id: activity.asset_id,
        activity_type: activity.activity_type,
        activity_date: activity.activity_date.to_rfc3339(),
        currency: activity.currency,
    }
}

/// Commit a single activity draft.
pub struct CommitActivityDraft;

#[async_trait::async_trait]
impl AgentTool for CommitActivityDraft {
    fn name(&self) -> &'static str {
        "commit_activity_draft"
    }

    fn description(&self) -> &'static str {
        "Persist a single reviewed activity draft (the shape returned by \
         record_activity) as a real activity. This MUTATES data — only call it \
         after the draft has been reviewed and confirmed. An existing assetId is \
         written to as-is (or the call fails); an ambiguous symbol fails; a new \
         asset is created only with createAssetIfMissing: true. Pass dryRun: true \
         to see the target asset without writing."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "draft": activity_draft_schema(),
                "dryRun": dry_run_schema()
            },
            "required": ["draft"]
        })
    }

    fn required_scopes(&self) -> &'static [AgentScope] {
        &[AgentScope::ActivitiesDraft, AgentScope::ActivitiesWrite]
    }

    fn access_level(&self) -> AgentToolAccess {
        AgentToolAccess::Write
    }

    fn sanitize_args_for_audit(&self, args: &serde_json::Value) -> serde_json::Value {
        redact_drafts(args)
    }

    async fn call(
        &self,
        env: Arc<dyn AgentEnvironment>,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Args {
            draft: CommitDraftInput,
            #[serde(default)]
            dry_run: bool,
        }
        let args: Args = serde_json::from_value(args)?;

        let assets = env
            .asset_service()
            .get_assets()
            .map_err(|e| AgentToolError::ExecutionFailed(e.to_string()))?;
        let plan = resolve_asset_plan(&assets, &args.draft)?;
        let new_activity = draft_to_new_activity(
            &args.draft.draft,
            &plan,
            args.draft.idempotency_key.as_deref(),
        )?;
        if args.dry_run {
            return Ok(AgentToolResult {
                content: serde_json::json!({ "dryRun": true, "planned": plan }),
            });
        }
        let created = env
            .activity_service()
            .create_activity(new_activity)
            .await
            .map_err(|e| AgentToolError::ExecutionFailed(e.to_string()))?;
        env.health_service().clear_cache().await;

        let output = CommitActivityDraftOutput {
            created: committed_summary(created),
        };
        Ok(AgentToolResult {
            content: serde_json::to_value(output)?,
        })
    }
}

/// Commit multiple activity drafts. Partial success: each row is created
/// independently and row-level failures are reported without aborting the rest.
pub struct CommitActivityDrafts;

#[async_trait::async_trait]
impl AgentTool for CommitActivityDrafts {
    fn name(&self) -> &'static str {
        "commit_activity_drafts"
    }

    fn description(&self) -> &'static str {
        "Persist multiple reviewed activity drafts (the shape returned by \
         record_activities) as real activities. This MUTATES data — only call it \
         after the drafts have been reviewed and confirmed. Each draft is created \
         independently; row-level failures are reported in `errors` without \
         aborting the others. Asset resolution and dryRun work as in \
         commit_activity_draft."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "drafts": {
                    "type": "array",
                    "description": "Activity drafts to persist.",
                    "maxItems": MAX_COMMIT_DRAFTS,
                    "items": activity_draft_schema()
                },
                "dryRun": dry_run_schema()
            },
            "required": ["drafts"]
        })
    }

    fn required_scopes(&self) -> &'static [AgentScope] {
        &[AgentScope::ActivitiesDraft, AgentScope::ActivitiesWrite]
    }

    fn access_level(&self) -> AgentToolAccess {
        AgentToolAccess::Write
    }

    fn sanitize_args_for_audit(&self, args: &serde_json::Value) -> serde_json::Value {
        redact_drafts(args)
    }

    async fn call(
        &self,
        env: Arc<dyn AgentEnvironment>,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        // Reject oversized batches from the raw array length before
        // deserializing the whole payload — a buggy or hostile client with
        // write scope must not be able to force unbounded work.
        if let Some(len) = args.get("drafts").and_then(|d| d.as_array()).map(Vec::len) {
            if len > MAX_COMMIT_DRAFTS {
                return Err(AgentToolError::InvalidInput(format!(
                    "Batch limited to {MAX_COMMIT_DRAFTS} drafts, got {len}"
                )));
            }
        }

        let args: CommitActivityDraftsArgs = serde_json::from_value(args)?;

        let activity_service = env.activity_service();
        let asset_service = env.asset_service();
        let load_assets = || {
            asset_service
                .get_assets()
                .map_err(|e| AgentToolError::ExecutionFailed(e.to_string()))
        };
        let mut assets = load_assets()?;
        let mut created = Vec::new();
        let mut planned = Vec::new();
        let mut errors = Vec::new();
        let mut any_created = false;

        for (index, input) in args.drafts.iter().enumerate() {
            let prepared = resolve_asset_plan(&assets, input).and_then(|plan| {
                draft_to_new_activity(&input.draft, &plan, input.idempotency_key.as_deref())
                    .map(|new| (plan, new))
            });
            let (plan, new_activity) = match prepared {
                Ok(prepared) => prepared,
                Err(e) => {
                    errors.push(CommitError {
                        index,
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            if args.dry_run {
                planned.push(PlannedCommit { index, plan });
                continue;
            }
            match activity_service.create_activity(new_activity).await {
                Ok(activity) => {
                    any_created = true;
                    created.push(committed_summary(activity));
                    // Later drafts for the same new symbol must see the asset
                    // this one just created.
                    if plan.asset_action == AssetAction::Create {
                        assets = load_assets()?;
                    }
                }
                Err(e) => errors.push(CommitError {
                    index,
                    message: e.to_string(),
                }),
            }
        }

        if args.dry_run {
            let output = CommitActivityDraftsDryRunOutput {
                dry_run: true,
                planned,
                errors,
            };
            return Ok(AgentToolResult {
                content: serde_json::to_value(output)?,
            });
        }

        if any_created {
            env.health_service().clear_cache().await;
        }

        let output = CommitActivityDraftsOutput { created, errors };
        Ok(AgentToolResult {
            content: serde_json::to_value(output)?,
        })
    }
}

fn dry_run_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "boolean",
        "description": "Resolve the target asset (assetId + assetAction: existing/create/none) without writing anything."
    })
}

/// JSON schema for an [`ActivityDraft`] commit input — the shape
/// `record_activity` returns under `draft`.
fn activity_draft_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "activityType": { "type": "string" },
            "activityDate": { "type": "string", "description": "ISO 8601 date." },
            "symbol": { "type": "string" },
            "assetId": { "type": "string" },
            "assetName": { "type": "string" },
            "quantity": { "type": "number" },
            "unitPrice": { "type": "number" },
            "amount": { "type": "number" },
            "fee": { "type": "number" },
            "tax": { "type": "number" },
            "currency": { "type": "string" },
            "accountId": { "type": "string" },
            "accountName": { "type": "string" },
            "subtype": { "type": "string" },
            "notes": { "type": "string" },
            "priceSource": { "type": "string" },
            "pricingMode": { "type": "string" },
            "isCustomAsset": { "type": "boolean" },
            "assetKind": { "type": "string" },
            "idempotencyKey": {
                "type": "string",
                "description": "Dedupe key such as the broker order number. A second commit with the same key fails and names the existing activity."
            },
            "createAssetIfMissing": {
                "type": "boolean",
                "description": "Allow creating a new asset when no existing asset matches the symbol. Default false."
            }
        },
        "required": ["activityType", "activityDate", "currency", "accountId"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wealthfolio_core::assets::InstrumentType;

    /// A fully-populated security buy draft; tweak fields per test.
    fn security_draft() -> ActivityDraft {
        ActivityDraft {
            activity_type: "BUY".to_string(),
            activity_date: "2024-01-15T00:00:00Z".to_string(),
            symbol: Some("AAPL".to_string()),
            asset_id: Some("SEC:AAPL:XNAS".to_string()),
            asset_name: Some("Apple Inc.".to_string()),
            quantity: Some(10.0),
            unit_price: Some(150.25),
            amount: Some(1502.5),
            fee: Some(1.0),
            tax: None,
            currency: "USD".to_string(),
            account_id: Some("acct-1".to_string()),
            account_name: Some("Brokerage".to_string()),
            subtype: None,
            notes: Some("from agent".to_string()),
            price_source: "user".to_string(),
            pricing_mode: "MARKET".to_string(),
            is_custom_asset: false,
            asset_kind: None,
        }
    }

    fn existing_plan(asset_id: &str) -> AssetPlan {
        AssetPlan {
            asset_id: Some(asset_id.to_string()),
            asset_action: AssetAction::Existing,
        }
    }

    fn create_plan() -> AssetPlan {
        AssetPlan {
            asset_id: None,
            asset_action: AssetAction::Create,
        }
    }

    fn input(draft: ActivityDraft) -> CommitDraftInput {
        CommitDraftInput {
            draft,
            idempotency_key: None,
            create_asset_if_missing: false,
        }
    }

    fn asset(id: &str, symbol: &str, instrument_type: InstrumentType) -> Asset {
        Asset {
            id: id.to_string(),
            display_code: Some(symbol.to_string()),
            instrument_symbol: Some(symbol.to_string()),
            instrument_type: Some(instrument_type),
            kind: AssetKind::Investment,
            is_active: true,
            ..Default::default()
        }
    }

    #[test]
    fn existing_asset_is_passed_by_id_only() {
        let new = draft_to_new_activity(&security_draft(), &existing_plan("bond-1"), None).unwrap();
        let asset = new.asset.expect("existing asset should be carried");
        assert_eq!(asset.id.as_deref(), Some("bond-1"));
        // No symbol/kind/quote mode for core to re-resolve by.
        assert!(asset.symbol.is_none());
        assert!(asset.kind.is_none());
        assert!(asset.quote_mode.is_none());
    }

    #[test]
    fn idempotency_key_is_forwarded() {
        let new =
            draft_to_new_activity(&security_draft(), &create_plan(), Some(" ORD-123 ")).unwrap();
        assert_eq!(new.idempotency_key.as_deref(), Some("ORD-123"));
        let new = draft_to_new_activity(&security_draft(), &create_plan(), Some("  ")).unwrap();
        assert!(new.idempotency_key.is_none());
    }

    #[test]
    fn plan_uses_existing_asset_id_even_when_symbol_is_shared() {
        let assets = vec![
            asset("dup", "BYMA-CAC5O", InstrumentType::Equity),
            asset("orig", "BYMA-CAC5O", InstrumentType::Bond),
        ];
        let mut draft = security_draft();
        draft.symbol = Some("BYMA-CAC5O".to_string());
        draft.asset_id = Some("orig".to_string());
        assert_eq!(
            resolve_asset_plan(&assets, &input(draft)).unwrap(),
            existing_plan("orig")
        );
    }

    #[test]
    fn plan_rejects_asset_id_that_contradicts_symbol() {
        let assets = vec![asset("orig", "BYMA-CAC5O", InstrumentType::Bond)];
        let mut draft = security_draft();
        draft.symbol = Some("AAPL".to_string());
        draft.asset_id = Some("orig".to_string());
        assert!(matches!(
            resolve_asset_plan(&assets, &input(draft)),
            Err(AgentToolError::InvalidInput(_))
        ));
    }

    #[test]
    fn plan_rejects_ambiguous_symbol_and_names_candidates() {
        let assets = vec![
            asset("dup", "BYMA-CAC5O", InstrumentType::Equity),
            asset("orig", "BYMA-CAC5O", InstrumentType::Bond),
        ];
        let mut draft = security_draft();
        draft.symbol = Some("BYMA-CAC5O".to_string());
        draft.asset_id = None;
        let err = resolve_asset_plan(&assets, &input(draft)).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("dup") && message.contains("orig"),
            "{message}"
        );
    }

    #[test]
    fn plan_resolves_unique_symbol_and_ignores_provider_derived_id() {
        let assets = vec![Asset {
            instrument_exchange_mic: Some("XNAS".to_string()),
            ..asset("aapl-uuid", "AAPL", InstrumentType::Equity)
        }];
        // record_activity emits "SEC:AAPL:XNAS"-style ids for search hits.
        assert_eq!(
            resolve_asset_plan(&assets, &input(security_draft())).unwrap(),
            existing_plan("aapl-uuid")
        );
    }

    #[test]
    fn plan_creates_only_when_explicitly_allowed() {
        let mut draft = security_draft();
        draft.symbol = Some("NU".to_string());
        draft.asset_id = Some("NU:XNYS".to_string());
        assert!(matches!(
            resolve_asset_plan(&[], &input(draft.clone())),
            Err(AgentToolError::InvalidInput(_))
        ));
        let allowed = CommitDraftInput {
            create_asset_if_missing: true,
            ..input(draft)
        };
        assert_eq!(resolve_asset_plan(&[], &allowed).unwrap(), create_plan());
    }

    #[test]
    fn plan_rejects_unknown_asset_id_without_symbol() {
        let mut draft = security_draft();
        draft.symbol = None;
        draft.asset_id = Some("missing".to_string());
        assert!(matches!(
            resolve_asset_plan(&[], &input(draft)),
            Err(AgentToolError::InvalidInput(_))
        ));
    }

    #[test]
    fn commit_input_accepts_commit_only_fields() {
        let parsed: CommitDraftInput = serde_json::from_value(serde_json::json!({
            "activityType": "SELL",
            "activityDate": "2026-09-11",
            "currency": "USD",
            "accountId": "acct-1",
            "priceSource": "user",
            "pricingMode": "MANUAL",
            "isCustomAsset": false,
            "idempotencyKey": "ORD-9",
            "createAssetIfMissing": true
        }))
        .unwrap();
        assert_eq!(parsed.idempotency_key.as_deref(), Some("ORD-9"));
        assert!(parsed.create_asset_if_missing);
        assert_eq!(parsed.draft.activity_type, "SELL");
    }

    #[test]
    fn maps_security_draft_to_new_activity() {
        let new = draft_to_new_activity(&security_draft(), &create_plan(), None).unwrap();
        assert_eq!(new.account_id, "acct-1");
        assert_eq!(new.activity_type, "BUY");
        assert_eq!(new.currency, "USD");
        assert_eq!(new.quantity, Some(Decimal::from_f64(10.0).unwrap()));
        assert_eq!(new.unit_price, Some(Decimal::from_f64(150.25).unwrap()));
        assert_eq!(new.fee, Some(Decimal::from_f64(1.0).unwrap()));
        assert_eq!(new.tax, None);
        let asset = new.asset.expect("security draft should carry an asset");
        assert!(asset.id.is_none());
        assert_eq!(asset.symbol.as_deref(), Some("AAPL"));
        // Never minted on commit — the service assigns it.
        assert!(new.id.is_none());
    }

    #[test]
    fn missing_account_id_is_rejected() {
        let mut draft = security_draft();
        draft.account_id = None;
        assert!(matches!(
            draft_to_new_activity(&draft, &create_plan(), None),
            Err(AgentToolError::InvalidInput(_))
        ));

        // Whitespace-only is treated as missing too.
        draft.account_id = Some("   ".to_string());
        assert!(matches!(
            draft_to_new_activity(&draft, &create_plan(), None),
            Err(AgentToolError::InvalidInput(_))
        ));
    }

    #[test]
    fn cash_draft_carries_no_asset() {
        let mut draft = security_draft();
        draft.activity_type = "DEPOSIT".to_string();
        draft.symbol = None;
        draft.asset_id = None;
        draft.quantity = None;
        draft.unit_price = None;
        let plan = AssetPlan {
            asset_id: None,
            asset_action: AssetAction::None,
        };
        let new = draft_to_new_activity(&draft, &plan, None).unwrap();
        assert!(
            new.asset.is_none(),
            "pure cash activity must carry no asset"
        );
        assert_eq!(new.amount, Some(Decimal::from_f64(1502.5).unwrap()));
    }

    #[test]
    fn non_finite_numeric_is_rejected() {
        let mut draft = security_draft();
        draft.quantity = Some(f64::NAN);
        assert!(matches!(
            draft_to_new_activity(&draft, &create_plan(), None),
            Err(AgentToolError::InvalidInput(_))
        ));

        let mut draft = security_draft();
        draft.amount = Some(f64::INFINITY);
        assert!(matches!(
            draft_to_new_activity(&draft, &create_plan(), None),
            Err(AgentToolError::InvalidInput(_))
        ));
    }

    #[test]
    fn audit_redaction_drops_draft_payloads() {
        let single = serde_json::json!({ "draft": { "amount": 1502.5, "notes": "secret" } });
        assert_eq!(
            redact_drafts(&single)["draft"],
            serde_json::json!("[redacted]")
        );

        let batch = serde_json::json!({ "drafts": [ { "amount": 1.0 }, { "amount": 2.0 } ] });
        assert_eq!(
            redact_drafts(&batch)["drafts"],
            serde_json::json!("[2 drafts]")
        );
    }

    #[test]
    fn audit_redaction_drops_unknown_keys() {
        // An extra top-level field must not survive into the audit log.
        let args = serde_json::json!({
            "draft": { "amount": 1.0 },
            "note": "SSN 123-45-6789",
        });
        let redacted = redact_drafts(&args);
        assert!(redacted.get("note").is_none());
        assert_eq!(redacted.as_object().unwrap().len(), 1);
    }

    #[test]
    fn commit_drafts_schema_caps_batch_size() {
        let schema = CommitActivityDrafts.input_schema();
        assert_eq!(
            schema["properties"]["drafts"]["maxItems"],
            serde_json::json!(MAX_COMMIT_DRAFTS)
        );
    }
}
