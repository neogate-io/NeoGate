# Billing Guide

NeoGate supports both internal gateway usage and paid service usage.

## Modes

| Mode | Behavior |
| --- | --- |
| Internal mode | Calls can be allowed without requiring a positive balance. Usage and cost are still recorded for analysis and chargeback. |
| Billing mode | Users or projects need available credit before calling paid models. Usage records drive balance updates and payment workflows. |

## What Gets Tracked

- User
- Project
- Project API key
- Provider
- Channel
- Model
- Token usage
- Request cost
- Balance changes

## Recommended Setup

1. Configure provider channels and upstream credentials.
2. Add model pricing before opening access to users.
3. Create projects for teams, customers, or applications.
4. Issue project API keys instead of sharing upstream vendor keys.
5. Monitor usage records and balance movements during the first production rollout.

For deeper implementation details, see [Billing Outbox](design/billing-outbox.md).

