//! Output parity snapshots for the read-tool catalog.
//!
//! These snapshots pin the exact JSON each read tool returns for fixed
//! fixture inputs. They exist to prove the agent-tools extraction
//! (`crates/agent-tools`) is behavior-preserving: captured BEFORE the
//! migration against the rig `Tool` impls, they must pass UNCHANGED
//! afterwards when the same tools run through the `AgentTool` adapter.
//!
//! Companion to `tool_schemas.rs` (which pins the input schemas). If one
//! of these fails during the migration, the migration changed observable
//! tool behavior — fix the migration, do not accept the snapshot.

#![cfg(feature = "test-utils")]

use chrono::{DateTime, NaiveDate, Utc};
use rig::tool::ToolDyn;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::sync::RwLock;
use wealthfolio_agent_tools::AgentTool;
use wealthfolio_ai::env::test_env::{
    MockAccountService, MockActivityService, MockAssetService, MockCashActivityService,
    MockEnvironment, MockHoldingsService, MockQuoteService, MockTaxonomyService,
};
use wealthfolio_ai::tools::{
    GetAccounts, GetAssetAllocation, GetAssetTaxonomyAssignments, GetCashBalances, GetGoals,
    GetHealthStatus, GetHoldings, GetIncome, GetPerformance, GetValuationHistory,
    ListAssetTaxonomies, ListCategorizationContext, PrepareAssetClassification, RecordActivities,
    RecordActivity, RigAgentTool, SearchActivities,
};
use wealthfolio_core::accounts::{Account, TrackingMode};
use wealthfolio_core::activities::{Activity, ActivityDetails, ActivityStatus};
use wealthfolio_core::assets::{Asset, AssetKind, InstrumentType, QuoteMode};
use wealthfolio_core::holdings::{Holding, HoldingType, Instrument, MonetaryValue};
use wealthfolio_core::quotes::SymbolSearchResult;
use wealthfolio_core::taxonomies::{
    AssetTaxonomyAssignment, Category, Taxonomy, TaxonomyWithCategories,
};
use wealthfolio_spending::cash_activities::model::CashFlowBucket;
use wealthfolio_spending::cash_activities::CashActivity;

fn fixture_account(id: &str, name: &str, account_type: &str, currency: &str) -> Account {
    let ts = NaiveDate::from_ymd_opt(2024, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    Account {
        id: id.to_string(),
        name: name.to_string(),
        account_type: account_type.to_string(),
        group: None,
        currency: currency.to_string(),
        is_default: false,
        is_active: true,
        created_at: ts,
        updated_at: ts,
        platform_id: None,
        account_number: None,
        meta: None,
        provider: None,
        provider_account_id: None,
        is_archived: false,
        tracking_mode: TrackingMode::Transactions,
    }
}

/// Mock environment with two seeded accounts; everything else default.
fn env() -> Arc<MockEnvironment> {
    let mut env = MockEnvironment::new();
    env.account_service = Arc::new(MockAccountService {
        accounts: vec![
            fixture_account("acc-1", "Brokerage", "SECURITIES", "USD"),
            fixture_account("acc-2", "Savings", "CASH", "EUR"),
        ],
    });
    Arc::new(env)
}

/// Call a migrated tool exactly as a rig agent would — through the
/// `RigAgentTool` adapter (`ToolDyn`), with args as a JSON string —
/// normalized to the same `{"ok"|"err"}` shape as `call_json`.
/// rig prefixes adapter errors with "ToolCallError: "; strip it so error
/// snapshots stay comparable with the pre-migration captures.
async fn call_json_dyn(
    tool: impl AgentTool + 'static,
    env: Arc<MockEnvironment>,
    args: serde_json::Value,
) -> serde_json::Value {
    let adapter = RigAgentTool::new(Arc::new(tool), env);
    match adapter.call(args.to_string()).await {
        Ok(output) => serde_json::json!({
            "ok": serde_json::from_str::<serde_json::Value>(&output).unwrap()
        }),
        Err(e) => {
            let msg = e.to_string();
            let msg = msg.strip_prefix("ToolCallError: ").unwrap_or(&msg);
            serde_json::json!({ "err": msg })
        }
    }
}

macro_rules! output_test_dyn {
    ($test_name:ident, $tool:expr, $args:tt) => {
        #[tokio::test]
        async fn $test_name() {
            let result = call_json_dyn($tool, env(), serde_json::json!($args)).await;
            insta::assert_json_snapshot!(result);
        }
    };
}

/// Like `output_test_dyn!` but with an explicit (seeded) environment.
macro_rules! output_test_dyn_env {
    ($test_name:ident, $tool:expr, $env:expr, $args:tt) => {
        #[tokio::test]
        async fn $test_name() {
            let result = call_json_dyn($tool, $env, serde_json::json!($args)).await;
            insta::assert_json_snapshot!(result);
        }
    };
}

output_test_dyn!(output_get_accounts, GetAccounts, {});
output_test_dyn!(
    output_get_accounts_compact,
    GetAccounts,
    { "displayMode": "compact" }
);
output_test_dyn!(output_get_cash_balances, GetCashBalances, {});
output_test_dyn!(output_get_holdings, GetHoldings, {});
output_test_dyn!(
    output_get_holdings_scoped,
    GetHoldings,
    { "accountId": "acc-1", "viewMode": "table" }
);
output_test_dyn!(output_get_asset_allocation, GetAssetAllocation, {});
output_test_dyn!(
    output_get_asset_allocation_by_sector,
    GetAssetAllocation,
    { "groupBy": "sector" }
);
output_test_dyn!(
    output_get_asset_allocation_by_region,
    GetAssetAllocation,
    { "groupBy": "region" }
);
output_test_dyn!(
    output_get_asset_allocation_by_risk,
    GetAssetAllocation,
    { "groupBy": "risk" }
);
output_test_dyn!(
    output_get_asset_allocation_invalid_group_by,
    GetAssetAllocation,
    { "groupBy": "invalid" }
);
output_test_dyn!(
    output_get_asset_allocation_drill_down,
    GetAssetAllocation,
    { "groupBy": "sector", "taxonomyId": "industries_gics", "categoryId": "TECHNOLOGY" }
);
output_test_dyn!(
    output_get_performance,
    GetPerformance,
    { "period": "1Y" }
);
output_test_dyn!(
    output_get_performance_scoped,
    GetPerformance,
    { "accountId": "acc-1", "period": "1M" }
);
output_test_dyn!(
    output_get_valuation_history,
    GetValuationHistory,
    { "startDate": "2024-01-01", "endDate": "2024-03-01" }
);
output_test_dyn!(
    output_get_valuation_history_scoped,
    GetValuationHistory,
    { "accountId": "acc-1", "startDate": "2024-01-01", "endDate": "2024-12-31" }
);
output_test_dyn!(
    output_search_activities,
    SearchActivities,
    { "dateFrom": "2024-01-01", "dateTo": "2024-12-31" }
);
output_test_dyn!(
    output_search_activities_filtered,
    SearchActivities,
    { "activityType": "DIVIDEND", "dateFrom": "2024-01-01", "pageSize": 25 }
);
output_test_dyn!(
    output_search_activities_invalid_date,
    SearchActivities,
    { "dateFrom": "2024-13-01" }
);
output_test_dyn!(output_get_income, GetIncome, { "period": "ALL" });
output_test_dyn!(output_get_income_default, GetIncome, {});
output_test_dyn!(output_get_goals, GetGoals, {});
output_test_dyn!(output_get_health_status, GetHealthStatus, {});
output_test_dyn!(output_list_asset_taxonomies, ListAssetTaxonomies, {});
output_test_dyn!(
    output_get_asset_taxonomy_assignments,
    GetAssetTaxonomyAssignments,
    { "assetQuery": "AAPL" }
);
output_test_dyn!(
    output_list_categorization_context,
    ListCategorizationContext,
    {}
);

// ---------------------------------------------------------------------------
// Classification fixtures (seeded envs) — pin the asset-resolution and
// taxonomy-filtering behaviors that used to be covered by inline unit tests
// in `crates/ai/src/tools/asset_classification.rs`.
// ---------------------------------------------------------------------------

fn fixture_ts() -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(2024, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
}

fn fixture_asset(
    id: &str,
    display_code: &str,
    symbol: &str,
    exchange_mic: Option<&str>,
    name: &str,
    is_active: bool,
) -> Asset {
    Asset {
        id: id.to_string(),
        kind: AssetKind::Investment,
        name: Some(name.to_string()),
        display_code: Some(display_code.to_string()),
        is_active,
        quote_mode: QuoteMode::Market,
        quote_ccy: "USD".to_string(),
        instrument_type: Some(InstrumentType::Equity),
        instrument_symbol: Some(symbol.to_string()),
        instrument_exchange_mic: exchange_mic.map(str::to_string),
        created_at: fixture_ts(),
        updated_at: fixture_ts(),
        ..Default::default()
    }
}

fn fixture_taxonomy(
    id: &str,
    name: &str,
    scope: &str,
    categories: Vec<Category>,
) -> TaxonomyWithCategories {
    TaxonomyWithCategories {
        taxonomy: Taxonomy {
            id: id.to_string(),
            name: name.to_string(),
            color: "#2563eb".to_string(),
            description: None,
            is_system: false,
            is_single_select: false,
            sort_order: 1,
            created_at: fixture_ts(),
            updated_at: fixture_ts(),
            scope: scope.to_string(),
        },
        categories,
    }
}

fn fixture_category(taxonomy_id: &str, id: &str, name: &str) -> Category {
    Category {
        id: id.to_string(),
        taxonomy_id: taxonomy_id.to_string(),
        parent_id: None,
        name: name.to_string(),
        key: name.to_lowercase().replace(' ', "_"),
        color: "#64748b".to_string(),
        description: None,
        sort_order: 1,
        created_at: fixture_ts(),
        updated_at: fixture_ts(),
        icon: None,
    }
}

fn fixture_child_category(taxonomy_id: &str, id: &str, parent_id: &str, name: &str) -> Category {
    Category {
        parent_id: Some(parent_id.to_string()),
        ..fixture_category(taxonomy_id, id, name)
    }
}

fn fixture_assignment(
    id: &str,
    asset_id: &str,
    taxonomy_id: &str,
    category_id: &str,
    weight: i32,
    source: &str,
) -> AssetTaxonomyAssignment {
    AssetTaxonomyAssignment {
        id: id.to_string(),
        asset_id: asset_id.to_string(),
        taxonomy_id: taxonomy_id.to_string(),
        category_id: category_id.to_string(),
        weight,
        source: source.to_string(),
        created_at: fixture_ts(),
        updated_at: fixture_ts(),
    }
}

/// Env seeded with active/inactive assets (including ambiguous tickers and
/// names), two asset-scoped taxonomies that share a category ID, one
/// activity-scoped taxonomy, and assignments for AAPL in both asset
/// taxonomies.
fn classification_env() -> Arc<MockEnvironment> {
    let mut env = MockEnvironment::new();
    env.asset_service = Arc::new(MockAssetService {
        assets: vec![
            fixture_asset(
                "asset-aapl",
                "AAPL",
                "AAPL",
                Some("XNAS"),
                "Apple Inc.",
                true,
            ),
            fixture_asset(
                "asset-aple",
                "APLE",
                "APLE",
                Some("XNYS"),
                "Apple Hospitality REIT",
                true,
            ),
            fixture_asset(
                "asset-shop",
                "SHOP",
                "SHOP",
                Some("XTSE"),
                "Shopify Inc.",
                true,
            ),
            fixture_asset(
                "asset-vt-xnas",
                "VT",
                "VT",
                Some("XNAS"),
                "Vanguard Total World Stock Index Fund ETF Shares",
                true,
            ),
            fixture_asset(
                "asset-vt-arcx",
                "VT",
                "VT",
                Some("ARCX"),
                "Vanguard Total World Stock Index Fund ETF Shares",
                true,
            ),
            fixture_asset(
                "asset-msft",
                "MSFT",
                "MSFT",
                Some("XNAS"),
                "Microsoft Corp.",
                false,
            ),
        ],
    });
    env.taxonomy_service = Arc::new(MockTaxonomyService {
        taxonomies: vec![
            fixture_taxonomy(
                "asset-tax",
                "Asset Class",
                "asset",
                vec![
                    fixture_category("asset-tax", "equity", "Equity"),
                    fixture_child_category("asset-tax", "equity-us", "equity", "US Equity"),
                    fixture_category("asset-tax", "cash", "Cash"),
                ],
            ),
            fixture_taxonomy(
                "factor-tax",
                "Factors",
                "asset",
                // Same category ID as asset-tax's "equity" on purpose:
                // assignment enrichment must resolve by (taxonomy, category).
                vec![fixture_category("factor-tax", "equity", "Value")],
            ),
            fixture_taxonomy(
                "activity-tax",
                "Spending",
                "activity",
                vec![fixture_category("activity-tax", "food", "Food")],
            ),
        ],
        assignments: vec![
            fixture_assignment(
                "assignment-1",
                "asset-aapl",
                "asset-tax",
                "equity",
                10000,
                "manual",
            ),
            fixture_assignment(
                "assignment-2",
                "asset-aapl",
                "factor-tax",
                "equity",
                10000,
                "ai",
            ),
        ],
    });
    Arc::new(env)
}

output_test_dyn_env!(
    output_list_asset_taxonomies_seeded_summaries,
    ListAssetTaxonomies,
    classification_env(),
    {}
);
output_test_dyn_env!(
    output_list_asset_taxonomies_seeded_root_categories,
    ListAssetTaxonomies,
    classification_env(),
    { "taxonomyId": "asset-tax", "includeCategories": true }
);
output_test_dyn_env!(
    output_list_asset_taxonomies_seeded_all_categories,
    ListAssetTaxonomies,
    classification_env(),
    { "taxonomyName": "Asset Class", "includeCategories": true, "categoryDepth": "all" }
);
output_test_dyn_env!(
    output_list_asset_taxonomies_invalid_depth,
    ListAssetTaxonomies,
    classification_env(),
    { "taxonomyId": "asset-tax", "includeCategories": true, "categoryDepth": "bogus" }
);
output_test_dyn_env!(
    output_list_asset_taxonomies_unknown_taxonomy,
    ListAssetTaxonomies,
    classification_env(),
    { "taxonomyId": "missing-tax" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_filtered,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "AAPL", "taxonomyId": "asset-tax" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_all_taxonomies,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "asset-aapl" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_name_match,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "Apple Inc." }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_fuzzy_name,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "Hospitality" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_provider_suffix,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "SHOP.TO" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_symbol_mic,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "VT XNAS" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_candidate_label,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "VT - Vanguard Total World Stock Index Fund ETF Shares (mic: ARCX, currency: USD, id: asset-vt-arcx)" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_ambiguous_symbol,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "VT" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_ambiguous_name,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "Apple" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_inactive_asset,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "MSFT" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_unknown_taxonomy,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "AAPL", "taxonomyId": "missing-tax" }
);
output_test_dyn_env!(
    output_get_asset_taxonomy_assignments_empty_query,
    GetAssetTaxonomyAssignments,
    classification_env(),
    { "assetQuery": "  " }
);

// ---------------------------------------------------------------------------
// Categorization-context fixtures (seeded env) — pin the deterministic
// rules/history pass through `list_categorization_context`.
// ---------------------------------------------------------------------------

fn fixture_cash_activity(
    id: &str,
    account_id: &str,
    activity_date: &str,
    notes: &str,
    assigned_category: Option<(&str, &str)>,
) -> CashActivity {
    let now = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let activity_date = DateTime::parse_from_rfc3339(activity_date)
        .unwrap()
        .with_timezone(&Utc);
    let assignments = assigned_category
        .map(|(taxonomy_id, category_id)| {
            vec![
                wealthfolio_spending::activity_assignments::ActivityTaxonomyAssignment {
                    id: format!("{id}-asg"),
                    activity_id: id.to_string(),
                    taxonomy_id: taxonomy_id.to_string(),
                    category_id: category_id.to_string(),
                    weight: 10_000,
                    source: "manual".to_string(),
                    created_at: now.naive_utc(),
                    updated_at: now.naive_utc(),
                },
            ]
        })
        .unwrap_or_default();
    CashActivity {
        activity: Activity {
            id: id.to_string(),
            account_id: account_id.to_string(),
            asset_id: None,
            activity_type: "WITHDRAWAL".to_string(),
            activity_type_override: None,
            source_type: None,
            subtype: None,
            status: ActivityStatus::Posted,
            activity_date,
            settlement_date: None,
            quantity: None,
            unit_price: None,
            amount: Some(Decimal::new(-1250, 2)),
            fee: None,
            tax: None,
            currency: "USD".to_string(),
            fx_rate: None,
            notes: Some(notes.to_string()),
            metadata: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
            import_run_id: None,
            is_user_modified: false,
            needs_review: false,
            created_at: now,
            updated_at: now,
        },
        cash_flow_bucket: CashFlowBucket::Spending,
        assignments,
        splits: Vec::new(),
        event_id: None,
        transfer_link_status: None,
        // Matches the -12.50 posted withdrawal above. Unused by the parity
        // assertions, but the fixture should not claim a movement the activity
        // does not describe.
        net_amount: -12.5,
        net_amount_base: None,
    }
}

/// Env seeded with one activity-scope taxonomy, one uncategorized cash row,
/// and one categorized row with the same normalized payee (history signal).
fn categorization_env() -> Arc<MockEnvironment> {
    let mut env = MockEnvironment::new();
    env.account_service = Arc::new(MockAccountService {
        accounts: vec![fixture_account("acc-1", "Brokerage", "SECURITIES", "USD")],
    });
    env.taxonomy_service = Arc::new(MockTaxonomyService {
        taxonomies: vec![fixture_taxonomy(
            "spending",
            "Spending",
            "activity",
            vec![
                fixture_category("spending", "cat-food", "Food"),
                fixture_child_category("spending", "cat-coffee", "cat-food", "Coffee"),
            ],
        )],
        assignments: vec![],
    });
    env.cash_activity_service = Arc::new(MockCashActivityService {
        items: vec![
            fixture_cash_activity(
                "cash-a",
                "acc-1",
                "2024-06-15T00:00:00Z",
                "SQ *COFFEE SHOP TORONTO",
                None,
            ),
            fixture_cash_activity(
                "cash-b",
                "acc-1",
                "2024-05-10T00:00:00Z",
                "SQ *COFFEE SHOP OTTAWA",
                Some(("spending", "cat-coffee")),
            ),
        ],
    });
    Arc::new(env)
}

output_test_dyn_env!(
    output_list_categorization_context_seeded,
    ListCategorizationContext,
    categorization_env(),
    {}
);
output_test_dyn_env!(
    output_list_categorization_context_explicit_ids,
    ListCategorizationContext,
    categorization_env(),
    { "activityIds": ["cash-a"] }
);

// ---------------------------------------------------------------------------
// Draft/suggest tool outputs (migrated from `crates/ai/src/tools/*.rs` inline
// tests). These pin the editable-draft behavior the assistant relies on.
// ---------------------------------------------------------------------------

/// Env with a single account so account auto-selection applies (the common
/// record_activity flow). No quote results, so symbols fall back to custom.
fn single_account_env() -> Arc<MockEnvironment> {
    let mut env = MockEnvironment::new();
    env.account_service = Arc::new(MockAccountService {
        accounts: vec![fixture_account("acc-1", "Main Broker", "SECURITIES", "USD")],
    });
    Arc::new(env)
}

output_test_dyn_env!(
    output_record_activity_buy,
    RecordActivity,
    single_account_env(),
    {
        "activityType": "BUY",
        "symbol": "AAPL",
        "activityDate": "2026-01-17",
        "quantity": 20.0,
        "unitPrice": 240.0
    }
);
output_test_dyn_env!(
    output_record_activity_deposit,
    RecordActivity,
    single_account_env(),
    { "activityType": "DEPOSIT", "activityDate": "2026-01-17", "amount": 5000.0 }
);
output_test_dyn_env!(
    output_record_activity_dividend_drip,
    RecordActivity,
    single_account_env(),
    {
        "activityType": "DIVIDEND",
        "symbol": "VTI",
        "activityDate": "2026-01-17",
        "quantity": 2.0,
        "subtype": "DRIP"
    }
);
output_test_dyn_env!(
    output_record_activities_mixed,
    RecordActivities,
    single_account_env(),
    {
        "activities": [
            { "activityType": "DEPOSIT", "activityDate": "2026-01-17", "amount": 1000.0 },
            { "activityType": "DEPOSIT", "activityDate": "2026-01-17" }
        ]
    }
);

/// A holding fixture; only the fields get_holdings reads are meaningful.
fn fixture_holding(id: &str, symbol: &str, holding_type: HoldingType, value: i64) -> Holding {
    let is_cash = holding_type == HoldingType::Cash;
    Holding {
        id: id.to_string(),
        account_id: "acc-1".to_string(),
        holding_type,
        is_closed: false,
        instrument: (!is_cash).then(|| Instrument {
            id: format!("asset-{symbol}"),
            symbol: symbol.to_string(),
            name: None,
            currency: "USD".to_string(),
            notes: None,
            pricing_mode: "MARKET".to_string(),
            preferred_provider: None,
            exchange_mic: None,
            instrument_type: None,
            classifications: None,
        }),
        asset_kind: None,
        quantity: Decimal::ONE,
        open_date: None,
        lots: None,
        contract_multiplier: Decimal::ONE,
        local_currency: "USD".to_string(),
        base_currency: "USD".to_string(),
        fx_rate: None,
        market_value: MonetaryValue {
            local: Decimal::from(value),
            base: Decimal::from(value),
        },
        cost_basis: None,
        price: None,
        purchase_price: None,
        unrealized_gain: None,
        unrealized_gain_pct: None,
        realized_gain: None,
        realized_gain_pct: None,
        total_gain: None,
        total_gain_pct: None,
        income: None,
        total_return: None,
        total_return_pct: None,
        return_basis: None,
        day_change: None,
        day_change_pct: None,
        prev_close_value: None,
        weight: Decimal::ZERO,
        as_of_date: NaiveDate::from_ymd_opt(2026, 7, 13).unwrap(),
        metadata: None,
        source_account_ids: Vec::new(),
    }
}

/// Three securities plus one cash row (cash is excluded from the output and
/// must not count toward pagination totals).
fn holdings_env() -> Arc<MockEnvironment> {
    let mut env = MockEnvironment::new();
    env.account_service = Arc::new(MockAccountService {
        accounts: vec![fixture_account("acc-1", "Brokerage", "SECURITIES", "USD")],
    });
    env.holdings_service = Arc::new(MockHoldingsService {
        holdings: vec![
            fixture_holding("h-1", "AAA", HoldingType::Security, 300),
            fixture_holding("h-2", "BBB", HoldingType::Security, 200),
            fixture_holding("h-cash", "CASH", HoldingType::Cash, 50),
            fixture_holding("h-3", "CCC", HoldingType::Security, 100),
        ],
    });
    Arc::new(env)
}

output_test_dyn_env!(
    output_get_holdings_page_1,
    GetHoldings,
    holdings_env(),
    { "viewMode": "table", "pageSize": 2 }
);
output_test_dyn_env!(
    output_get_holdings_page_2,
    GetHoldings,
    holdings_env(),
    { "viewMode": "table", "page": 2, "pageSize": 2 }
);
output_test_dyn_env!(
    output_get_holdings_all_fit,
    GetHoldings,
    holdings_env(),
    { "viewMode": "table" }
);

/// Two activities on distinct assets that share the symbol "XS123".
fn fixture_activity_details(id: &str, asset_id: &str, created_at: &str) -> ActivityDetails {
    ActivityDetails {
        id: id.to_string(),
        account_id: "acc-1".to_string(),
        asset_id: asset_id.to_string(),
        activity_type: "BUY".to_string(),
        subtype: None,
        status: ActivityStatus::Posted,
        date: "2024-03-01T00:00:00Z".to_string(),
        quantity: Some("10".to_string()),
        unit_price: Some("99.5".to_string()),
        currency: "EUR".to_string(),
        fee: Some("0".to_string()),
        tax: None,
        amount: Some("995".to_string()),
        needs_review: false,
        comment: None,
        fx_rate: None,
        created_at: created_at.to_string(),
        updated_at: created_at.to_string(),
        account_name: "Brokerage".to_string(),
        account_currency: "USD".to_string(),
        asset_symbol: "XS123".to_string(),
        asset_name: Some("Corp Bond".to_string()),
        exchange_mic: None,
        asset_pricing_mode: "MANUAL".to_string(),
        instrument_type: Some("BOND".to_string()),
        asset_contract_multiplier: None,
        source_system: None,
        source_record_id: None,
        source_group_id: None,
        idempotency_key: None,
        import_run_id: None,
        is_user_modified: false,
        metadata: None,
    }
}

fn activities_env() -> Arc<MockEnvironment> {
    let mut env = MockEnvironment::new();
    env.account_service = Arc::new(MockAccountService {
        accounts: vec![fixture_account("acc-1", "Brokerage", "SECURITIES", "USD")],
    });
    env.activity_service = Arc::new(MockActivityService {
        activities: vec![
            fixture_activity_details("act-1", "bond-a", "2024-03-01T10:00:00+00:00"),
            fixture_activity_details("act-2", "bond-b", "2024-03-02T11:00:00+00:00"),
        ],
    });
    Arc::new(env)
}

output_test_dyn_env!(
    output_search_activities_asset_ids,
    SearchActivities,
    activities_env(),
    { "pageSize": 1 }
);
output_test_dyn_env!(
    output_search_activities_symbol_and_asset_id_conflict,
    SearchActivities,
    activities_env(),
    { "symbol": "XS123", "assetId": "bond-a" }
);

/// The symbol search resolves to an existing custom MANUAL-priced bond.
fn existing_manual_bond_env() -> Arc<MockEnvironment> {
    let mut env = MockEnvironment::new();
    env.account_service = Arc::new(MockAccountService {
        accounts: vec![fixture_account("acc-1", "Main Broker", "SECURITIES", "USD")],
    });
    env.asset_service = Arc::new(MockAssetService {
        assets: vec![Asset {
            id: "bond-a".to_string(),
            kind: AssetKind::Investment,
            name: Some("Corp Bond 2030".to_string()),
            display_code: Some("XS123".to_string()),
            is_active: true,
            quote_mode: QuoteMode::Manual,
            quote_ccy: "EUR".to_string(),
            instrument_type: Some(InstrumentType::Bond),
            created_at: fixture_ts(),
            updated_at: fixture_ts(),
            ..Default::default()
        }],
    });
    env.quote_service = Arc::new(MockQuoteService {
        search_results: RwLock::new(vec![SymbolSearchResult {
            symbol: "XS123".to_string(),
            short_name: "Corp Bond 2030".to_string(),
            long_name: "Corp Bond 2030".to_string(),
            quote_type: "BOND".to_string(),
            currency: Some("EUR".to_string()),
            quote_mode: Some("MANUAL".to_string()),
            is_existing: true,
            existing_asset_id: Some("bond-a".to_string()),
            score: 100.0,
            ..Default::default()
        }]),
    });
    Arc::new(env)
}

output_test_dyn_env!(
    output_record_activity_existing_manual_bond,
    RecordActivity,
    existing_manual_bond_env(),
    {
        "activityType": "BUY",
        "symbol": "XS123",
        "activityDate": "2026-01-17",
        "quantity": 10.0,
        "unitPrice": 99.5
    }
);

// prepare_asset_classification — reuse the seeded classification env.
output_test_dyn_env!(
    output_prepare_asset_classification_draft,
    PrepareAssetClassification,
    classification_env(),
    {
        "assetQuery": "AAPL",
        "taxonomyId": "asset-tax",
        "assignments": [
            { "categoryId": "equity", "weightBasisPoints": 6000, "sourceLabel": "Equity" },
            { "categoryId": "cash", "weightBasisPoints": 3000, "sourceLabel": "Cash" }
        ]
    }
);
output_test_dyn_env!(
    output_prepare_asset_classification_ambiguous,
    PrepareAssetClassification,
    classification_env(),
    {
        "assetQuery": "VT",
        "taxonomyId": "asset-tax",
        "assignments": [
            { "categoryId": "equity", "weightBasisPoints": 9000, "sourceLabel": "Equity" }
        ]
    }
);
output_test_dyn_env!(
    output_prepare_asset_classification_duplicate_error,
    PrepareAssetClassification,
    classification_env(),
    {
        "assetQuery": "AAPL",
        "taxonomyId": "asset-tax",
        "assignments": [
            { "categoryId": "equity", "weightBasisPoints": 5000, "sourceLabel": "Equity" },
            { "categoryId": "equity", "weightBasisPoints": 5000, "sourceLabel": "Equity" }
        ]
    }
);
