use super::*;
use ibapi::contracts::{Contract, Currency, SecurityType, Symbol};
use ibapi::orders::{CommissionReport, Execution, ExecutionData};

fn opt_contract(strike: f64, right: &str, expiry: &str) -> Contract {
    Contract {
        symbol: Symbol("TSLA".to_string()),
        security_type: SecurityType::Option,
        last_trade_date_or_contract_month: expiry.to_string(),
        strike,
        right: right.to_string(),
        multiplier: "100".to_string(),
        currency: Currency("USD".to_string()),
        local_symbol: "TSLA  260504C00390000".to_string(),
        ..Default::default()
    }
}

fn stk_contract(symbol: &str) -> Contract {
    Contract {
        symbol: Symbol(symbol.to_string()),
        security_type: SecurityType::Stock,
        currency: Currency("USD".to_string()),
        local_symbol: symbol.to_string(),
        ..Default::default()
    }
}

fn execution(
    exec_id: &str,
    time: &str,
    side: &str,
    shares: f64,
    avg_price: f64,
    order_id: i32,
    account: &str,
) -> Execution {
    Execution {
        order_id,
        execution_id: exec_id.to_string(),
        time: time.to_string(),
        account_number: account.to_string(),
        side: side.to_string(),
        shares,
        average_price: avg_price,
        ..Default::default()
    }
}

fn fill(contract: Contract, exec: Execution) -> IBExecutions {
    IBExecutions::ExecutionData(ExecutionData {
        request_id: 1,
        contract,
        execution: exec,
    })
}

fn report(exec_id: &str, commission: f64, realized: Option<f64>) -> IBExecutions {
    IBExecutions::CommissionReport(CommissionReport {
        execution_id: exec_id.to_string(),
        commission,
        currency: "USD".to_string(),
        realized_pnl: realized,
        yields: None,
        yield_redemption_date: String::new(),
    })
}

fn target() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, 4).unwrap()
}

#[test]
fn executions_merges_commission_into_matching_fill() {
    let events = vec![
        fill(
            stk_contract("AAPL"),
            execution("e1", "20260504 10:30:00", "BOT", 100.0, 150.25, 1001, "DU1"),
        ),
        fill(
            stk_contract("MSFT"),
            execution("e2", "20260504 11:15:00", "SLD", 50.0, 420.0, 1002, "DU1"),
        ),
        report("e1", 1.05, Some(0.0)),
        report("e2", 0.65, Some(125.50)),
    ];

    let result = merge_commission_reports(events, target());

    assert_eq!(result.rows.len(), 2);
    assert!(result.orphan_commission_ids.is_empty());

    let aapl = result.rows.iter().find(|r| r.exec_id == "e1").unwrap();
    assert_eq!(aapl.commission, Some(1.05));
    assert_eq!(aapl.realized_pnl, Some(0.0));
    assert_eq!(aapl.commission_currency.as_deref(), Some("USD"));

    let msft = result.rows.iter().find(|r| r.exec_id == "e2").unwrap();
    assert_eq!(msft.commission, Some(0.65));
    assert_eq!(msft.realized_pnl, Some(125.50));
}

#[test]
fn executions_handles_fill_without_commission_report() {
    let events = vec![
        fill(
            stk_contract("AAPL"),
            execution("e1", "20260504 10:30:00", "BOT", 100.0, 150.25, 1001, "DU1"),
        ),
        fill(
            stk_contract("MSFT"),
            execution("e2", "20260504 11:15:00", "SLD", 50.0, 420.0, 1002, "DU1"),
        ),
        report("e1", 1.05, None),
    ];

    let result = merge_commission_reports(events, target());

    assert_eq!(result.rows.len(), 2);
    let e1 = result.rows.iter().find(|r| r.exec_id == "e1").unwrap();
    assert_eq!(e1.commission, Some(1.05));
    let e2 = result.rows.iter().find(|r| r.exec_id == "e2").unwrap();
    assert_eq!(e2.commission, None);
    assert_eq!(e2.realized_pnl, None);
    assert!(result.orphan_commission_ids.is_empty());
}

#[test]
fn executions_preserves_option_contract_fields() {
    let events = vec![fill(
        opt_contract(390.0, "C", "20260504"),
        execution("e1", "20260504 10:30:00", "BOT", 1.0, 2.50, 1001, "DU1"),
    )];

    let result = merge_commission_reports(events, target());

    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_eq!(row.contract_type, "OPT");
    assert_eq!(row.expiry, NaiveDate::from_ymd_opt(2026, 5, 4));
    assert_eq!(row.strike, Some(390.0));
    assert_eq!(row.right.as_deref(), Some("C"));
    assert_eq!(row.multiplier.as_deref(), Some("100"));
}

#[test]
fn executions_stock_fill_has_empty_option_fields() {
    let events = vec![fill(
        stk_contract("RDDT"),
        execution("e1", "20260504 10:30:00", "BOT", 100.0, 50.0, 1001, "DU1"),
    )];

    let result = merge_commission_reports(events, target());

    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_eq!(row.contract_type, "STK");
    assert_eq!(row.expiry, None);
    assert_eq!(row.strike, None);
    assert_eq!(row.right, None);
    assert_eq!(row.multiplier, None);
}

#[test]
fn executions_filters_near_midnight_et() {
    let make_events = || {
        vec![fill(
            stk_contract("AAPL"),
            execution("e1", "20260504 23:59:30", "BOT", 100.0, 150.0, 1001, "DU1"),
        )]
    };

    let included = merge_commission_reports(make_events(), target());
    assert_eq!(included.rows.len(), 1);
    assert_eq!(included.rows[0].exec_id, "e1");

    let next_day = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
    let excluded = merge_commission_reports(make_events(), next_day);
    assert!(excluded.rows.is_empty());
}

#[test]
fn executions_drops_orphan_commission_report_with_warn() {
    let events = vec![
        fill(
            stk_contract("AAPL"),
            execution("e1", "20260504 10:30:00", "BOT", 100.0, 150.0, 1001, "DU1"),
        ),
        report("ghost", 0.99, Some(10.0)),
    ];

    let result = merge_commission_reports(events, target());

    // Real fill survives; orphan does not produce a row.
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].exec_id, "e1");
    assert_eq!(result.orphan_commission_ids, vec!["ghost".to_string()]);
}

#[test]
fn executions_normalises_right_call_put() {
    let events = vec![fill(
        opt_contract(100.0, "CALL", "20260504"),
        execution("e1", "20260504 10:30:00", "BOT", 1.0, 1.0, 1001, "DU1"),
    )];

    let result = merge_commission_reports(events, target());
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].right.as_deref(), Some("C"));

    let put_events = vec![fill(
        opt_contract(100.0, "PUT", "20260504"),
        execution("e2", "20260504 10:30:00", "SLD", 1.0, 1.0, 1002, "DU1"),
    )];
    let put_result = merge_commission_reports(put_events, target());
    assert_eq!(put_result.rows[0].right.as_deref(), Some("P"));
}

#[test]
fn executions_late_commission_first_then_fill() {
    // CommissionReport arriving before its ExecutionData should still
    // merge cleanly — order-of-arrival within a drain is not guaranteed.
    let events = vec![
        report("e1", 1.05, Some(50.0)),
        fill(
            stk_contract("AAPL"),
            execution("e1", "20260504 10:30:00", "BOT", 100.0, 150.0, 1001, "DU1"),
        ),
    ];

    let result = merge_commission_reports(events, target());
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].commission, Some(1.05));
    assert_eq!(result.rows[0].realized_pnl, Some(50.0));
}

#[test]
fn executions_populates_account_number() {
    let events = vec![fill(
        stk_contract("AAPL"),
        execution(
            "e1",
            "20260504 10:30:00",
            "BOT",
            100.0,
            150.0,
            1001,
            "U7654321",
        ),
    )];

    let result = merge_commission_reports(events, target());
    assert_eq!(result.rows[0].account, "U7654321");
}

#[test]
fn executions_sorts_rows_ascending_by_time() {
    let events = vec![
        fill(
            stk_contract("AAPL"),
            execution("e2", "20260504 11:30:00", "BOT", 1.0, 1.0, 1002, "DU1"),
        ),
        fill(
            stk_contract("AAPL"),
            execution("e1", "20260504 10:00:00", "BOT", 1.0, 1.0, 1001, "DU1"),
        ),
        fill(
            stk_contract("AAPL"),
            execution("e3", "20260504 13:45:00", "BOT", 1.0, 1.0, 1003, "DU1"),
        ),
    ];

    let result = merge_commission_reports(events, target());
    let ids: Vec<_> = result.rows.iter().map(|r| r.exec_id.as_str()).collect();
    assert_eq!(ids, vec!["e1", "e2", "e3"]);
}

#[test]
fn parse_option_expiry_handles_yyyymm_as_none() {
    assert_eq!(
        parse_option_expiry("20260504"),
        NaiveDate::from_ymd_opt(2026, 5, 4)
    );
    assert_eq!(parse_option_expiry("202605"), None);
    assert_eq!(parse_option_expiry(""), None);
}
