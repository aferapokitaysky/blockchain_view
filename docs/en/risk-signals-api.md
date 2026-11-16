# Risk signal API

These endpoints store analyst-entered evidence for a tracked address. They do not fetch sanctions lists, query commercial screening providers, or calculate or update the UI's provisional exposure score.

The API has no authentication yet and is intended for local development only. Do not expose it to untrusted networks.

## Endpoints

The `{investigation_id}` and `{address_id}` path values must identify an address that belongs to that investigation.

| Method | Path | Result |
| --- | --- | --- |
| `GET` | `/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals` | List up to 500 signals, newest observation first. |
| `POST` | `/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals` | Create a signal; returns `201 Created`. |
| `GET` | `/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals/{signal_id}` | Read one signal. |
| `PUT` | `/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals/{signal_id}` | Replace all editable fields; returns `200 OK`. |
| `DELETE` | `/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals/{signal_id}` | Delete one signal; returns `204 No Content`. |

## Request body

`POST` and `PUT` require a JSON object with every field below:

```json
{
  "signal_key": "manual_service_exposure",
  "category": "exposure",
  "severity": "medium",
  "source_name": "Analyst review",
  "source_reference": "case-42/evidence-2",
  "observed_at": "2026-11-15T10:00:00Z",
  "details": {
    "hops": 2,
    "note": "Analyst-entered observation"
  }
}
```

- `signal_key`: non-empty, at most 120 characters.
- `category`: `sanctions`, `exposure`, `behavior`, or `other`.
- `severity`: `critical`, `high`, `medium`, `low`, or `informational`.
- `source_name`: required, non-empty, at most 120 characters.
- `source_reference`: required, non-empty, at most 512 characters. It can be a URL or a provider's record identifier.
- `observed_at`: required RFC 3339 timestamp. Timestamps more than five minutes in the future are rejected.
- `details`: required non-empty JSON object, at most 8 KiB when serialized.
- The complete request body is limited to 16 KiB. Unknown fields are rejected.

Text fields are trimmed before storage. The API preserves the submitted source name and reference; it does not verify their truth, fetch the cited record, or claim a sanctions match.

## Response and errors

Responses include `id`, `tracked_address_id`, `signal_key`, `category`, `severity`, `source_name`, `source_reference`, `observed_at`, `details`, and `created_at`. These provenance fields let clients display who or what supplied the signal and when it was observed.

Application errors use the shared `{ "error": { "code": "…", "message": "…" } }` envelope:

- `400 invalid_json`: malformed JSON or missing JSON content type.
- `404 not_found`: the address does not belong to the investigation, or the signal is not scoped to both path IDs.
- `413 payload_too_large`: the request body exceeds 16 KiB.
- `422 invalid_request`: a required field, enum, timestamp, or details object is invalid.
- `500 internal_error`: storage failure; internal database details are logged server-side only.

## Scope and data use

All reads and mutations constrain the signal through the `tracked_addresses` row using both investigation and address IDs. A mismatch returns `404`; it does not reveal whether the address or signal exists elsewhere. The signal list is capped at 500 rows per address.

Signals are stored as analyst-provided evidence. The API does not merge duplicate reports, adjudicate an address, call external providers, or change the frontend demo score. Source licensing, retention, analyst identity, and authentication are not handled yet.
