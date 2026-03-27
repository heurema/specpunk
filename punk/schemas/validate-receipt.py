#!/usr/bin/env python3
"""Validate a receipt JSON file against receipt.v1.schema.json.

No external dependencies — uses stdlib json only.
Checks required fields, types, enum values, and additionalProperties.

Usage:
    python3 validate-receipt.py receipt.json
    python3 validate-receipt.py --schema path/to/schema.json receipt.json
    echo '{"schema_version":1,...}' | python3 validate-receipt.py -

Exit codes: 0 = valid, 1 = invalid, 2 = error
"""
import json
import sys
import os
from datetime import datetime


def load_schema(schema_path=None):
    if schema_path is None:
        schema_path = os.path.join(os.path.dirname(__file__), "receipt.v1.schema.json")
    with open(schema_path) as f:
        return json.load(f)


def validate(receipt, schema):
    errors = []

    # Check type
    if not isinstance(receipt, dict):
        return ["receipt must be a JSON object"]

    # Check required fields
    for field in schema.get("required", []):
        if field not in receipt:
            errors.append(f"missing required field: {field}")

    props = schema.get("properties", {})

    # Check each field
    for key, value in receipt.items():
        if key not in props:
            if schema.get("additionalProperties") is False:
                errors.append(f"unexpected field: {key}")
            continue

        spec = props[key]

        # Type check
        expected_types = spec.get("type", [])
        if isinstance(expected_types, str):
            expected_types = [expected_types]

        type_ok = False
        for t in expected_types:
            if t == "string" and isinstance(value, str):
                type_ok = True
            elif t == "integer" and isinstance(value, int) and not isinstance(value, bool):
                type_ok = True
            elif t == "number" and isinstance(value, (int, float)) and not isinstance(value, bool):
                type_ok = True
            elif t == "boolean" and isinstance(value, bool):
                type_ok = True
            elif t == "array" and isinstance(value, list):
                type_ok = True
            elif t == "object" and isinstance(value, dict):
                type_ok = True
            elif t == "null" and value is None:
                type_ok = True

        if not type_ok:
            errors.append(f"{key}: expected type {expected_types}, got {type(value).__name__}")
            continue

        # Const check
        if "const" in spec and value != spec["const"]:
            errors.append(f"{key}: expected {spec['const']}, got {value}")

        # Enum check
        if "enum" in spec and value not in spec["enum"]:
            errors.append(f"{key}: value '{value}' not in {spec['enum']}")

        # MinLength check
        if "minLength" in spec and isinstance(value, str) and len(value) < spec["minLength"]:
            errors.append(f"{key}: string too short (min {spec['minLength']})")

        # Minimum check
        if "minimum" in spec and isinstance(value, (int, float)) and value < spec["minimum"]:
            errors.append(f"{key}: value {value} below minimum {spec['minimum']}")

        # Format check (date-time)
        if spec.get("format") == "date-time" and isinstance(value, str):
            try:
                # Accept both Z and +00:00 suffixes
                v = value.replace("Z", "+00:00")
                datetime.fromisoformat(v)
            except ValueError:
                errors.append(f"{key}: invalid date-time format: {value}")

        # Array items type check
        if isinstance(value, list) and "items" in spec:
            item_type = spec["items"].get("type")
            for i, item in enumerate(value):
                if item_type == "string" and not isinstance(item, str):
                    errors.append(f"{key}[{i}]: expected string, got {type(item).__name__}")

    return errors


def main():
    schema_path = None
    receipt_path = None

    args = sys.argv[1:]
    i = 0
    while i < len(args):
        if args[i] == "--schema" and i + 1 < len(args):
            schema_path = args[i + 1]
            i += 2
        else:
            receipt_path = args[i]
            i += 1

    if receipt_path is None:
        print("Usage: validate-receipt.py [--schema schema.json] receipt.json", file=sys.stderr)
        sys.exit(2)

    try:
        schema = load_schema(schema_path)
    except (FileNotFoundError, json.JSONDecodeError) as e:
        print(f"schema error: {e}", file=sys.stderr)
        sys.exit(2)

    try:
        if receipt_path == "-":
            receipt = json.load(sys.stdin)
        else:
            with open(receipt_path) as f:
                receipt = json.load(f)
    except (FileNotFoundError, json.JSONDecodeError) as e:
        print(f"receipt error: {e}", file=sys.stderr)
        sys.exit(2)

    errors = validate(receipt, schema)

    if errors:
        for err in errors:
            print(f"  error: {err}", file=sys.stderr)
        sys.exit(1)
    else:
        sys.exit(0)


if __name__ == "__main__":
    main()
