use super::validate::{
    validate_email, validate_hex_string, validate_instrument_id, validate_phone, validate_price,
    validate_quantity, validate_symbol, validate_timestamp, validate_uuid, EmailValidator,
    EnumValidator, FieldValidator, MessageValidator, NumericRangeValidator, RegexValidator,
    RequiredValidator, Severity, StringLengthValidator, ValidationResult,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn error_codes(result: &ValidationResult) -> Vec<&str> {
    result.errors.iter().map(|error| error.code.as_str()).collect()
}

fn assert_error(result: &ValidationResult, field: &str, code: &str) {
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.field == field && error.code == code),
        "expected error {field}:{code}, got {:?}",
        result.errors
    );
}

// ---------------------------------------------------------------------------
// SCHEMA VALIDATION STAGE
// ---------------------------------------------------------------------------

#[test]
fn message_validator_new_reports_schema_mismatch_without_registered_schema() {
    let validator = MessageValidator::new();
    let result = validator.validate(0x1001, 3, br#"{"side":"buy"}"#);

    assert!(!result.valid);
    assert_error(&result, "_schema", "schema_mismatch");
}

#[test]
fn message_validator_handles_non_json_payload_without_panicking() {
    let validator = MessageValidator::new();
    let result = validator.validate(0x1001, 3, b"not valid json");

    assert!(!result.valid);
    assert_eq!(result.errors.len(), 1);
    assert_error(&result, "_schema", "schema_mismatch");
}

#[test]
fn validation_result_valid_constructor_has_no_errors_or_warnings() {
    let result = ValidationResult::valid();

    assert!(result.valid);
    assert!(!result.has_errors());
    assert!(!result.has_warnings());
}

#[test]
fn validation_result_error_constructor_sets_error_severity() {
    let result = ValidationResult::error("field", "bad", "bad field");

    assert!(!result.valid);
    assert!(result.has_errors());
    assert_eq!(result.errors[0].field, "field");
    assert_eq!(result.errors[0].code, "bad");
    assert_eq!(result.errors[0].message, "bad field");
    assert_eq!(result.errors[0].severity, Severity::Error);
}

#[test]
fn validation_result_combine_merges_errors_and_warnings() {
    let mut result = ValidationResult::valid();
    result.add_warning("first warning");

    let mut other = ValidationResult::valid();
    other.add_error("field", "invalid", "invalid field");
    other.add_warning("second warning");

    result.combine(other);

    assert!(!result.valid);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.warnings, vec!["first warning", "second warning"]);
    assert!(result.has_errors());
    assert!(result.has_warnings());
}

#[test]
fn validation_result_add_warning_does_not_invalidate_result() {
    let mut result = ValidationResult::valid();
    result.add_warning("deprecated field");

    assert!(result.valid);
    assert!(!result.has_errors());
    assert!(result.has_warnings());
}

// ---------------------------------------------------------------------------
// FIELD VALIDATION STAGE
// ---------------------------------------------------------------------------

#[test]
fn required_validator_accepts_some_and_rejects_none() {
    let validator = RequiredValidator;

    assert!(validator.validate(&Some("value"), "name").valid);
    let missing: Option<&str> = None;
    let result = validator.validate(&missing, "name");

    assert!(!result.valid);
    assert_error(&result, "name", "required");
}

#[test]
fn string_length_validator_accepts_boundary_lengths() {
    let validator = StringLengthValidator {
        min: Some(2),
        max: Some(4),
    };

    assert!(validator.validate(&"ab".to_string(), "code").valid);
    assert!(validator.validate(&"abcd".to_string(), "code").valid);
}

#[test]
fn string_length_validator_reports_min_and_max_violations() {
    let validator = StringLengthValidator {
        min: Some(2),
        max: Some(4),
    };

    assert_error(&validator.validate(&"a".to_string(), "code"), "code", "min_length");
    assert_error(
        &validator.validate(&"abcde".to_string(), "code"),
        "code",
        "max_length",
    );
}

#[test]
fn numeric_range_validator_accepts_boundaries_and_rejects_out_of_range() {
    let validator = NumericRangeValidator {
        min: Some(10.0),
        max: Some(20.0),
    };

    assert!(validator.validate(&10.0, "amount").valid);
    assert!(validator.validate(&20.0, "amount").valid);
    assert_error(&validator.validate(&9.99, "amount"), "amount", "min_value");
    assert_error(&validator.validate(&20.01, "amount"), "amount", "max_value");
}

#[test]
fn regex_validator_accepts_matching_value_and_rejects_mismatch() {
    let validator = RegexValidator {
        pattern: r"^[A-Z]{3}-\d{3}$",
    };

    assert!(validator.validate(&"ABC-123".to_string(), "ticket").valid);
    assert_error(
        &validator.validate(&"abc-123".to_string(), "ticket"),
        "ticket",
        "pattern_mismatch",
    );
}

#[test]
fn enum_validator_accepts_only_declared_variants() {
    let validator = EnumValidator {
        variants: &["market", "limit"],
    };

    assert!(validator.validate(&"market".to_string(), "type").valid);
    assert_error(
        &validator.validate(&"iceberg".to_string(), "type"),
        "type",
        "invalid_value",
    );
}

#[test]
fn email_validator_accepts_basic_email_and_rejects_invalid_format() {
    let validator = EmailValidator;

    assert!(validator.validate(&"trader@example.com".to_string(), "email").valid);
    assert_error(
        &validator.validate(&"trader.example.com".to_string(), "email"),
        "email",
        "invalid_email",
    );
}

#[test]
fn registered_field_validator_runs_for_matching_message_type() {
    let mut validator = MessageValidator::new();
    validator.register_field_validator(
        42,
        Box::new(|payload: &Value| match payload.get("name").and_then(Value::as_str) {
            Some(name) if !name.is_empty() => ValidationResult::valid(),
            _ => ValidationResult::error("name", "required", "name required"),
        }),
    );

    let result = validator.validate(42, 3, br#"{"name":""}"#);

    assert!(!result.valid);
    assert_error(&result, "_schema", "schema_mismatch");
    assert_error(&result, "name", "required");
}

#[test]
fn registered_field_validator_is_skipped_for_other_message_types() {
    let mut validator = MessageValidator::new();
    validator.register_field_validator(
        42,
        Box::new(|_| ValidationResult::error("name", "required", "name required")),
    );

    let result = validator.validate(43, 3, br#"{"name":""}"#);

    assert!(!result.valid);
    assert_error(&result, "_schema", "schema_mismatch");
    assert!(!error_codes(&result).contains(&"required"));
}

// ---------------------------------------------------------------------------
// BUSINESS VALIDATION STAGE
// ---------------------------------------------------------------------------

#[test]
fn validate_order_payload_accepts_valid_market_order_without_price() {
    let payload = json!({
        "side": "buy",
        "type": "market",
        "quantity": 10.5,
        "time_in_force": "ioc"
    });

    let result = MessageValidator::validate_order_payload(&payload);

    assert!(result.valid, "unexpected errors: {:?}", result.errors);
}

#[test]
fn validate_order_payload_accepts_valid_limit_order_with_price() {
    let payload = json!({
        "side": "sell",
        "type": "limit",
        "quantity": 1.0,
        "price": 99.95,
        "time_in_force": "gtc"
    });

    let result = MessageValidator::validate_order_payload(&payload);

    assert!(result.valid, "unexpected errors: {:?}", result.errors);
}

#[test]
fn validate_order_payload_reports_missing_required_fields() {
    let payload = json!({});
    let result = MessageValidator::validate_order_payload(&payload);

    assert!(!result.valid);
    assert_error(&result, "side", "required");
    assert_error(&result, "type", "required");
    assert_error(&result, "quantity", "required");
    assert_error(&result, "price", "required");
}

#[test]
fn validate_order_payload_rejects_invalid_side_and_type() {
    let payload = json!({
        "side": "hold",
        "type": "iceberg",
        "quantity": 1.0,
        "price": 10.0
    });

    let result = MessageValidator::validate_order_payload(&payload);

    assert!(!result.valid);
    assert_error(&result, "side", "invalid_side");
    assert_error(&result, "type", "invalid_type");
}

#[test]
fn validate_order_payload_rejects_quantity_range_violations() {
    let zero_quantity = json!({
        "side": "buy",
        "type": "market",
        "quantity": 0.0
    });
    let too_large_quantity = json!({
        "side": "buy",
        "type": "market",
        "quantity": 1_000_000.01
    });

    assert_error(
        &MessageValidator::validate_order_payload(&zero_quantity),
        "quantity",
        "invalid_quantity",
    );
    assert_error(
        &MessageValidator::validate_order_payload(&too_large_quantity),
        "quantity",
        "max_exceeded",
    );
}

#[test]
fn validate_order_payload_rejects_non_numeric_quantity_as_type_mismatch() {
    let payload = json!({
        "side": "buy",
        "type": "market",
        "quantity": "10"
    });

    let result = MessageValidator::validate_order_payload(&payload);

    assert!(!result.valid);
    assert_error(&result, "quantity", "required");
}

#[test]
fn validate_order_payload_requires_positive_price_for_non_market_orders() {
    let missing_price = json!({
        "side": "sell",
        "type": "limit",
        "quantity": 5.0
    });
    let zero_price = json!({
        "side": "sell",
        "type": "stop_limit",
        "quantity": 5.0,
        "price": 0.0
    });

    assert_error(
        &MessageValidator::validate_order_payload(&missing_price),
        "price",
        "required",
    );
    assert_error(
        &MessageValidator::validate_order_payload(&zero_price),
        "price",
        "invalid_price",
    );
}

#[test]
fn validate_order_payload_rejects_invalid_time_in_force() {
    let payload = json!({
        "side": "buy",
        "type": "market",
        "quantity": 1.0,
        "time_in_force": "gtt"
    });

    let result = MessageValidator::validate_order_payload(&payload);

    assert!(!result.valid);
    assert_error(&result, "time_in_force", "invalid_tif");
}

#[test]
fn validate_account_payload_accepts_valid_amount_and_currency() {
    let payload = json!({
        "amount": 2500.25,
        "currency": "USDC"
    });

    let result = MessageValidator::validate_account_payload(&payload);

    assert!(result.valid, "unexpected errors: {:?}", result.errors);
}

#[test]
fn validate_account_payload_rejects_amount_range_violations() {
    let negative_amount = json!({ "amount": -0.01, "currency": "USD" });
    let too_large_amount = json!({ "amount": 1_000_000_000.01, "currency": "USD" });

    assert_error(
        &MessageValidator::validate_account_payload(&negative_amount),
        "amount",
        "invalid_amount",
    );
    assert_error(
        &MessageValidator::validate_account_payload(&too_large_amount),
        "amount",
        "max_exceeded",
    );
}

#[test]
fn validate_account_payload_rejects_unsupported_currency() {
    let payload = json!({
        "amount": 25.0,
        "currency": "DOGE"
    });

    let result = MessageValidator::validate_account_payload(&payload);

    assert!(!result.valid);
    assert_error(&result, "currency", "invalid_currency");
}

#[test]
fn duplicated_business_rules_diverge_from_compliance_auditor_aml_threshold() {
    let compliance_source = include_str!("../../../compliance/ComplianceAuditor.java");
    assert!(
        compliance_source.contains("threshold = 10000.00")
            && compliance_source.contains("transaction_amount"),
        "ComplianceAuditor AML threshold changed; update this comparison"
    );

    let protocol_payload = json!({
        "amount": 10_000.01,
        "currency": "USD"
    });
    let protocol_result = MessageValidator::validate_account_payload(&protocol_payload);

    assert!(
        protocol_result.valid,
        "protocol account validation allows amounts until 1_000_000_000; got {:?}",
        protocol_result.errors
    );
    assert!(
        10_000.01_f64 > 10_000.00_f64,
        "the same amount would trip ComplianceAuditor.auditAML's hard-coded threshold"
    );
}

// ---------------------------------------------------------------------------
// INTEGRITY VALIDATION STAGE
// ---------------------------------------------------------------------------

#[test]
fn custom_validator_can_model_successful_checksum_validation() {
    let payload = br#"{"id":1}"#;
    let expected = Sha256::digest(payload).to_vec();
    let mut validator = MessageValidator::new();
    validator.register_custom_validator(Box::new(move |_message_type, bytes| {
        if Sha256::digest(bytes).to_vec() == expected {
            ValidationResult::valid()
        } else {
            ValidationResult::error("_checksum", "checksum_mismatch", "checksum mismatch")
        }
    }));

    let result = validator.validate(7, 3, payload);

    assert!(!result.valid, "schema still fails because no schema is registered");
    assert_error(&result, "_schema", "schema_mismatch");
    assert!(!error_codes(&result).contains(&"checksum_mismatch"));
}

#[test]
fn custom_validator_reports_checksum_failure() {
    let expected = Sha256::digest(b"trusted payload").to_vec();
    let mut validator = MessageValidator::new();
    validator.register_custom_validator(Box::new(move |_message_type, bytes| {
        if Sha256::digest(bytes).to_vec() == expected {
            ValidationResult::valid()
        } else {
            ValidationResult::error("_checksum", "checksum_mismatch", "checksum mismatch")
        }
    }));

    let result = validator.validate(7, 3, br#"{"id":1}"#);

    assert!(!result.valid);
    assert_error(&result, "_schema", "schema_mismatch");
    assert_error(&result, "_checksum", "checksum_mismatch");
}

#[test]
fn custom_validator_runs_even_for_non_json_payloads() {
    let mut validator = MessageValidator::new();
    validator.register_custom_validator(Box::new(|_message_type, bytes| {
        if bytes.starts_with(b"TOT") {
            ValidationResult::valid()
        } else {
            ValidationResult::error("_signature", "invalid_signature", "missing magic header")
        }
    }));

    let result = validator.validate(7, 3, b"bad-binary-payload");

    assert!(!result.valid);
    assert_error(&result, "_schema", "schema_mismatch");
    assert_error(&result, "_signature", "invalid_signature");
}

// ---------------------------------------------------------------------------
// CONVENIENCE VALIDATORS AND EDGE CASES
// ---------------------------------------------------------------------------

#[test]
fn validate_email_accepts_simple_address_and_rejects_missing_tld() {
    assert!(validate_email("alice.trader+alerts@example.co"));
    assert!(!validate_email("alice@example"));
}

#[test]
fn validate_phone_accepts_formatted_digits_between_10_and_15_digits() {
    assert!(validate_phone("+1 (555) 010-9999"));
    assert!(validate_phone("1234567890"));
    assert!(validate_phone("123456789012345"));
    assert!(!validate_phone("123456789"));
    assert!(!validate_phone("1234567890123456"));
}

#[test]
fn validate_uuid_requires_lowercase_hyphenated_uuid() {
    assert!(validate_uuid("123e4567-e89b-12d3-a456-426614174000"));
    assert!(!validate_uuid("123E4567-E89B-12D3-A456-426614174000"));
    assert!(!validate_uuid("123e4567e89b12d3a456426614174000"));
}

#[test]
fn validate_hex_string_checks_exact_byte_length_and_hex_digits() {
    assert!(validate_hex_string("00ffaa", 3));
    assert!(validate_hex_string("00FFAA", 3));
    assert!(!validate_hex_string("00ffaa", 4));
    assert!(!validate_hex_string("00ffag", 3));
}

#[test]
fn validate_timestamp_accepts_inclusive_protocol_window() {
    assert!(validate_timestamp(946_684_800_000));
    assert!(validate_timestamp(4_102_444_800_000));
    assert!(!validate_timestamp(946_684_799_999));
    assert!(!validate_timestamp(4_102_444_800_001));
}

#[test]
fn validate_symbol_requires_uppercase_base_and_quote_with_slash() {
    assert!(validate_symbol("BTC/USD"));
    assert!(validate_symbol("ETH2/USDC"));
    assert!(!validate_symbol("btc/USD"));
    assert!(!validate_symbol("BTCUSD"));
    assert!(!validate_symbol("TOO-LONG-SYMBOL/USD"));
}

#[test]
fn validate_instrument_id_requires_lowercase_alphanumeric_length_bounds() {
    assert!(validate_instrument_id("btcusd"));
    assert!(validate_instrument_id("ab"));
    assert!(!validate_instrument_id("a"));
    assert!(!validate_instrument_id("BTCUSD"));
    assert!(!validate_instrument_id("abcdefghijklmnopqrstu"));
}

#[test]
fn validate_price_enforces_positive_upper_bound_and_precision() {
    assert!(validate_price(0.000_000_001));
    assert!(validate_price(999_999_999.999));
    assert!(!validate_price(0.0));
    assert!(!validate_price(1_000_000_000.0));
    assert!(!validate_price(1.123_456_789_9));
}

#[test]
fn validate_quantity_enforces_positive_exclusive_upper_bound() {
    assert!(validate_quantity(0.000_000_001));
    assert!(validate_quantity(99_999_999.999));
    assert!(!validate_quantity(0.0));
    assert!(!validate_quantity(100_000_000.0));
}
