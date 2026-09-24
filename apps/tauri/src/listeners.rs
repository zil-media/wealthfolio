use futures::FutureExt;
use log::{error, info, warn};
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::AppHandle;
use tokio::task::JoinSet;
use wealthfolio_core::health::HealthServiceTrait;
use wealthfolio_core::portfolio::snapshot::{
    reconcile_quote_sync_from_latest_account_snapshots, snapshot_date_requires_remediation,
    SnapshotRecalcMode,
};
use wealthfolio_core::portfolio::valuation::ValuationRecalcMode;
use wealthfolio_core::quotes::SyncResult;
use wealthfolio_core::utils::time_utils::{parse_user_timezone_or_default, user_today};

use crate::context::ServiceContext;
use crate::events::{
    MarketSyncResult, PortfolioRequestPayload, MARKET_SYNC_COMPLETE, MARKET_SYNC_ERROR,
    MARKET_SYNC_START, PORTFOLIO_UPDATE_COMPLETE, PORTFOLIO_UPDATE_ERROR, PORTFOLIO_UPDATE_START,
};

/// Portfolio requests belong to one runtime, including their calculation phase.
pub struct PortfolioTasks(Mutex<Option<JoinSet<()>>>);

impl PortfolioTasks {
    pub fn new() -> Self {
        Self(Mutex::new(Some(JoinSet::new())))
    }

    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        let mut tasks = self.0.lock().unwrap();
        if let Some(tasks) = tasks.as_mut() {
            while tasks.try_join_next().is_some() {}
            tasks.spawn_on(task, tauri::async_runtime::handle().inner());
        }
    }

    pub async fn stop(&self) {
        // Taking the set also rejects late requests from commands already in flight.
        let tasks = self.0.lock().unwrap().take();
        if let Some(mut tasks) = tasks {
            tasks.shutdown().await;
        }
    }
}

fn resolve_listener_account_ids(
    context: &Arc<ServiceContext>,
    account_ids: Option<&Vec<String>>,
) -> Result<Vec<String>, wealthfolio_core::Error> {
    if let Some(target_ids) = account_ids {
        return Ok(target_ids.clone());
    }

    Ok(context
        .account_service()
        .get_non_archived_accounts()?
        .into_iter()
        .map(|account| account.id)
        .collect())
}

fn recalculation_modes(
    force_recalc: bool,
    since_date: Option<chrono::NaiveDate>,
    today: chrono::NaiveDate,
) -> (SnapshotRecalcMode, ValuationRecalcMode) {
    let safe_since_date =
        since_date.filter(|date| !snapshot_date_requires_remediation(*date, today));
    match safe_since_date {
        Some(date) => (
            SnapshotRecalcMode::SinceDate(date),
            ValuationRecalcMode::SinceDate(date),
        ),
        None if force_recalc => (SnapshotRecalcMode::Full, ValuationRecalcMode::Full),
        None => (
            SnapshotRecalcMode::IncrementalFromLast,
            ValuationRecalcMode::IncrementalFromLast,
        ),
    }
}

// Keep an unwinding provider panic inside the market-sync error path so the
// background task still emits a terminal event. Never expose the panic payload.
async fn run_market_sync(
    operation: impl Future<Output = wealthfolio_core::Result<SyncResult>>,
) -> wealthfolio_core::Result<SyncResult> {
    AssertUnwindSafe(operation)
        .catch_unwind()
        .await
        .unwrap_or_else(|_| {
            Err(wealthfolio_core::Error::Unexpected(
                "Price refresh stopped unexpectedly".to_string(),
            ))
        })
}

/// Handles the common logic for both portfolio update and recalculation requests.
pub(crate) fn handle_portfolio_request(
    handle: AppHandle,
    context: Arc<ServiceContext>,
    payload: PortfolioRequestPayload,
    force_recalc: bool,
) {
    if !context.is_active() {
        return;
    }
    let handle_clone = handle.clone(); // Clone handle for async block

    let task_context = Arc::clone(&context);
    context.portfolio_tasks.spawn(async move {
        let context = task_context;
        let market_sync_mode = payload.market_sync_mode.clone();
        let accounts_to_recalc = payload.account_ids.clone();
        let since_date = payload.since_date;
        {
            // Only perform market sync if the mode requires it
            if market_sync_mode.requires_sync() {
                let market_data_service = context.quote_service();
                let snapshot_service = context.snapshot_service();
                let account_ids_for_sync = resolve_listener_account_ids(&context, None)
                    .unwrap_or_else(|err| {
                        warn!(
                            "Failed to resolve accounts for quote sync reconciliation: {}",
                            err
                        );
                        Vec::new()
                    });

                if let Err(e) = reconcile_quote_sync_from_latest_account_snapshots(
                    snapshot_service.as_ref(),
                    market_data_service.as_ref(),
                    &account_ids_for_sync,
                )
                .await
                {
                    warn!(
                                "Failed to reconcile quote sync state from latest holdings: {}. Quote sync planning may be affected.",
                                e
                            );
                }

                // Emit sync start event
                if let Err(e) =
                    crate::events::emit_for_profile(&handle_clone, &context, MARKET_SYNC_START, &())
                {
                    error!("Failed to emit market:sync-start event: {}", e);
                }

                let sync_start = Instant::now();
                let asset_ids = market_sync_mode.asset_ids().cloned();

                // Convert MarketSyncMode to SyncMode for the quote service
                let sync_result = match market_sync_mode.to_sync_mode() {
                    Some(sync_mode) => {
                        run_market_sync(market_data_service.sync(sync_mode, asset_ids)).await
                    }
                    None => {
                        // This shouldn't happen since we checked requires_sync()
                        warn!("MarketSyncMode requires sync but returned None for SyncMode");
                        Ok(wealthfolio_core::quotes::SyncResult::default())
                    }
                };

                let sync_duration = sync_start.elapsed();
                info!("Market data sync completed in: {:?}", sync_duration);

                match sync_result {
                    Ok(result) => {
                        // Convert SyncResult to legacy format for backwards compatibility
                        let failed_syncs = result.failures;
                        let skipped_reasons = result
                            .skipped_reasons
                            .into_iter()
                            .map(|(asset_id, reason)| (asset_id, reason.to_string()))
                            .collect();

                        context.health_service().clear_cache().await;

                        let result_payload = MarketSyncResult {
                            failed_syncs,
                            skipped_reasons,
                            show_skipped_reasons: false,
                        };
                        if let Err(e) = crate::events::emit_for_profile(
                            &handle_clone,
                            &context,
                            MARKET_SYNC_COMPLETE,
                            &result_payload,
                        ) {
                            error!("Failed to emit market:sync-complete event: {}", e);
                        }
                        // Initialize the FxService after successful sync
                        let fx_service = context.fx_service();
                        if let Err(e) = fx_service.initialize() {
                            error!(
                                "Failed to initialize FxService after market data sync: {}",
                                e
                            );
                        }

                        // Trigger calculation after successful sync
                        let (snap_mode, val_mode) = recalculation_modes(
                            force_recalc,
                            since_date,
                            user_today(parse_user_timezone_or_default(&context.get_timezone())),
                        );
                        handle_portfolio_calculation(
                            handle_clone.clone(),
                            context.clone(),
                            accounts_to_recalc,
                            snap_mode,
                            val_mode,
                        ).await;
                    }
                    Err(e) => {
                        if let Err(e_emit) = crate::events::emit_for_profile(
                            &handle_clone,
                            &context,
                            MARKET_SYNC_ERROR,
                            &e.to_string(),
                        ) {
                            error!("Failed to emit market:sync-error event: {}", e_emit);
                        }
                        error!("Market data sync failed: {}. Skipping portfolio calculation for this request.", e);
                    }
                }
            } else {
                // MarketSyncMode::None - skip market sync, just recalculate
                info!("Skipping market sync (MarketSyncMode::None)");
                let (snap_mode, val_mode) = recalculation_modes(
                    force_recalc,
                    since_date,
                    user_today(parse_user_timezone_or_default(&context.get_timezone())),
                );
                handle_portfolio_calculation(
                    handle_clone.clone(),
                    context.clone(),
                    accounts_to_recalc,
                    snap_mode,
                    val_mode,
                ).await;
            }
        }
    });
}

// This function handles the portfolio snapshot and history calculation logic
async fn handle_portfolio_calculation(
    app_handle: AppHandle,
    context: Arc<ServiceContext>,
    account_ids_input: Option<Vec<String>>,
    snapshot_mode: SnapshotRecalcMode,
    valuation_mode: ValuationRecalcMode,
) {
    if let Err(e) =
        crate::events::emit_for_profile(&app_handle, &context, PORTFOLIO_UPDATE_START, ())
    {
        error!("Failed to emit {} event: {}", PORTFOLIO_UPDATE_START, e);
    }

    let account_service = context.account_service();
    let snapshot_service = context.snapshot_service();
    let valuation_service = context.valuation_service();

    // Step 0: Resolve account scope. Specific requests are processed as-is;
    // full recalculations rebuild every non-archived account, including closed accounts.
    let account_ids: Vec<String> = if let Some(target_ids) = account_ids_input {
        target_ids
    } else {
        match account_service.get_non_archived_accounts() {
            Ok(accounts) => accounts.into_iter().map(|a| a.id).collect(),
            Err(e) => {
                let err_msg = format!("Failed to list non-archived accounts: {}", e);
                error!("{}", err_msg);
                if let Err(e_emit) = crate::events::emit_for_profile(
                    &app_handle,
                    &context,
                    PORTFOLIO_UPDATE_ERROR,
                    &err_msg,
                ) {
                    error!(
                        "Failed to emit {} event: {}",
                        PORTFOLIO_UPDATE_ERROR, e_emit
                    );
                }
                return;
            }
        }
    };

    // --- Step 1: Calculate Account-Specific Snapshots ---
    if !account_ids.is_empty() {
        let account_snapshot_result = snapshot_service
            .recalculate_holdings_snapshots(Some(account_ids.as_slice()), snapshot_mode.clone())
            .await;

        if let Err(e) = account_snapshot_result {
            let err_msg = format!(
                "calculate_holdings_snapshots for targeted accounts failed: {}",
                e
            );
            error!("{}", err_msg);
            if let Err(e_emit) = crate::events::emit_for_profile(
                &app_handle,
                &context,
                PORTFOLIO_UPDATE_ERROR,
                &err_msg,
            ) {
                error!(
                    "Failed to emit {} event: {}",
                    PORTFOLIO_UPDATE_ERROR, e_emit
                );
            }
        }
    }

    // --- Step 2: Update position status from latest real-account snapshots ---
    let quote_service = context.quote_service();
    let quote_reconciliation_account_ids = resolve_listener_account_ids(&context, None)
        .unwrap_or_else(|err| {
            warn!(
                "Failed to resolve accounts for quote sync reconciliation: {}",
                err
            );
            Vec::new()
        });
    if let Err(e) = reconcile_quote_sync_from_latest_account_snapshots(
        snapshot_service.as_ref(),
        quote_service.as_ref(),
        &quote_reconciliation_account_ids,
    )
    .await
    {
        warn!(
                "Failed to update position status from holdings: {}. Quote sync planning may be affected.",
                e
            );
    }

    // --- Step 3: Calculate Valuation History ---
    let accounts_for_valuation = account_ids;

    if !accounts_for_valuation.is_empty() {
        match valuation_service
            .calculate_valuation_histories(&accounts_for_valuation, valuation_mode)
            .await
        {
            Ok(outcome) => {
                for failure in outcome.failures {
                    error!(
                        "Failed to calculate valuation history for account '{}': {}",
                        failure.account_id, failure.message
                    );
                    if let Err(emit_error) = crate::events::emit_for_profile(
                        &app_handle,
                        &context,
                        PORTFOLIO_UPDATE_ERROR,
                        &failure,
                    ) {
                        error!("Failed to emit portfolio error: {}", emit_error);
                    }
                }
            }
            Err(error) => {
                let message = format!("Failed to load shared valuation facts: {}", error);
                error!("{}", message);
                let _ = crate::events::emit_for_profile(
                    &app_handle,
                    &context,
                    PORTFOLIO_UPDATE_ERROR,
                    &message,
                );
            }
        }
    }

    context.health_service().clear_cache().await;

    if let Err(e) =
        crate::events::emit_for_profile(&app_handle, &context, PORTFOLIO_UPDATE_COMPLETE, ())
    {
        error!("Failed to emit {} event: {}", PORTFOLIO_UPDATE_COMPLETE, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn portfolio_requests_can_start_outside_a_tokio_thread() {
        let tasks = PortfolioTasks::new();
        let (started, receiver) = std::sync::mpsc::channel();
        tasks.spawn(async move {
            started.send(()).unwrap();
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        tauri::async_runtime::block_on(tasks.stop());
    }

    #[tokio::test]
    async fn locking_joins_portfolio_work_before_the_writer_closes() {
        let tasks = PortfolioTasks::new();
        let runtime_owner = Arc::new(());
        let task_owner = runtime_owner.clone();
        let (started, started_rx) = tokio::sync::oneshot::channel();
        tasks.spawn(async move {
            let _owner = task_owner;
            started.send(()).unwrap();
            // A refresh waiting on a provider must not retain the profile at lock.
            std::future::pending::<()>().await;
        });
        started_rx.await.unwrap();
        assert_eq!(Arc::strong_count(&runtime_owner), 2);
        tasks.stop().await;
        // This is the ownership condition teardown needs before closing the writer.
        assert_eq!(Arc::strong_count(&runtime_owner), 1);

        // An already-admitted command cannot launch new work after shutdown.
        let late_owner = runtime_owner.clone();
        tasks.spawn(async move {
            let _owner = late_owner;
            std::future::pending::<()>().await;
        });
        assert_eq!(Arc::strong_count(&runtime_owner), 1);
        tasks.stop().await;
    }

    #[tokio::test]
    async fn market_sync_panic_becomes_safe_error_and_allows_next_refresh() {
        let result = run_market_sync(async {
            tokio::task::yield_now().await;
            panic!("provider internal detail that must not reach the UI");
        })
        .await;

        match result {
            Err(wealthfolio_core::Error::Unexpected(message)) => {
                assert_eq!(message, "Price refresh stopped unexpectedly");
            }
            other => panic!("Expected a safe refresh error, got {other:?}"),
        }

        let next = run_market_sync(async {
            Ok(SyncResult {
                synced: 1,
                quotes_synced: 2,
                ..Default::default()
            })
        })
        .await
        .unwrap();
        assert_eq!(next.synced, 1);
        assert_eq!(next.quotes_synced, 2);
    }

    #[tokio::test]
    async fn market_sync_preserves_returned_errors() {
        let result = run_market_sync(async {
            Err(wealthfolio_core::Error::Unexpected(
                "Provider request timed out".to_string(),
            ))
        })
        .await;
        assert!(
            matches!(result, Err(wealthfolio_core::Error::Unexpected(message))
            if message == "Provider request timed out")
        );
    }

    #[test]
    fn dated_recalculation_request_uses_since_date_for_both_engines() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        let (snapshot_mode, valuation_mode) = recalculation_modes(true, Some(date), date);

        assert!(matches!(snapshot_mode, SnapshotRecalcMode::SinceDate(value) if value == date));
        assert!(matches!(valuation_mode, ValuationRecalcMode::SinceDate(value) if value == date));
    }

    #[test]
    fn invalid_dated_recalculation_request_falls_back_to_full() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        let invalid = NaiveDate::from_ymd_opt(224, 7, 20).unwrap();
        let (snapshot_mode, valuation_mode) = recalculation_modes(true, Some(invalid), today);

        assert!(matches!(snapshot_mode, SnapshotRecalcMode::Full));
        assert!(matches!(valuation_mode, ValuationRecalcMode::Full));
    }

    #[test]
    fn undated_force_recalculation_remains_full() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        let (snapshot_mode, valuation_mode) = recalculation_modes(true, None, today);

        assert!(matches!(snapshot_mode, SnapshotRecalcMode::Full));
        assert!(matches!(valuation_mode, ValuationRecalcMode::Full));
    }

    #[test]
    fn ordinary_undated_update_remains_incremental() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        let (snapshot_mode, valuation_mode) = recalculation_modes(false, None, today);

        assert!(matches!(
            snapshot_mode,
            SnapshotRecalcMode::IncrementalFromLast
        ));
        assert!(matches!(
            valuation_mode,
            ValuationRecalcMode::IncrementalFromLast
        ));
    }
}
